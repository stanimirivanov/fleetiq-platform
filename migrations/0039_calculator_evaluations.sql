-- Empty additive storage: no existing history is rewritten or reinterpreted.
-- Opaque input IDs are SHA-256 of the exact history primary key's JSON tuple.
-- They are lookup identities, not authenticity proofs; the trigger verifies facts.
CREATE FUNCTION analytics.semantic_evidence_id(t TEXT, m TEXT, p TEXT, v TEXT, o BIGINT, b TEXT, r NUMERIC, basis TEXT)
RETURNS TEXT LANGUAGE SQL IMMUTABLE STRICT AS $$
    SELECT encode(sha256(convert_to(jsonb_build_array(t,m,p,v,o,b,trunc(r)::text,basis)::text,'UTF8')),'hex')
$$;
CREATE INDEX ix_history_calculator_evidence ON history.asset_property_changes
    (tenant_id, analytics.semantic_evidence_id(tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis));
-- Minimal immutable snapshot pin; administration/activation remain a later slice.
CREATE TABLE analytics.evaluation_configurations (
    tenant_id TEXT NOT NULL,
    configuration_id TEXT NOT NULL CHECK(metadata.is_valid_identifier(configuration_id)),
    calculator_id TEXT NOT NULL,
    calculator_version TEXT NOT NULL,
    window_ms BIGINT NOT NULL CHECK(window_ms BETWEEN 1 AND 604800000),
    lateness_ms BIGINT NOT NULL CHECK(lateness_ms BETWEEN 0 AND 604800000),
    minimum_samples INTEGER NOT NULL CHECK(minimum_samples BETWEEN 1 AND 1024),
    PRIMARY KEY(tenant_id,configuration_id),
    FOREIGN KEY(tenant_id,calculator_id,calculator_version) REFERENCES analytics.calculator_definitions(tenant_id,calculator_id,calculator_version)
);
CREATE TRIGGER evaluation_configurations_immutable BEFORE UPDATE OR DELETE ON analytics.evaluation_configurations FOR EACH ROW EXECUTE FUNCTION analytics.prevent_definition_mutation();
CREATE TABLE analytics.evaluations (
    tenant_id TEXT NOT NULL REFERENCES metadata.tenants(tenant_id),
    evaluation_id TEXT NOT NULL CHECK(metadata.is_valid_identifier(evaluation_id)),
    replay_key JSONB NOT NULL,
    key_digest BYTEA NOT NULL CHECK(key_digest=sha256(convert_to(replay_key::text,'UTF8'))),
    scope_asset_id TEXT GENERATED ALWAYS AS (replay_key->>'scope') STORED NOT NULL,
    calculator_id TEXT GENERATED ALWAYS AS (replay_key->>'calculator') STORED NOT NULL,
    calculator_version TEXT GENERATED ALWAYS AS (replay_key->>'version') STORED NOT NULL,
    FOREIGN KEY (tenant_id,scope_asset_id) REFERENCES metadata.assets(tenant_id,asset_id),
    FOREIGN KEY (tenant_id,calculator_id,calculator_version) REFERENCES analytics.calculator_definitions(tenant_id,calculator_id,calculator_version),
    minimum_samples INTEGER NOT NULL CHECK(minimum_samples BETWEEN 1 AND 1024),
    evaluated_at_ms BIGINT NOT NULL,
    outcome TEXT NOT NULL CHECK(outcome IN ('complete','insufficient','incompatible_input','invalid_output','resource_limit')),
    outputs JSONB NOT NULL CHECK(jsonb_typeof(outputs)='array' AND jsonb_array_length(outputs)<=1024),
    PRIMARY KEY(tenant_id,evaluation_id),
    UNIQUE(tenant_id,key_digest),
    CHECK(outcome='complete' OR outputs='[]'::jsonb)
);
COMMENT ON TABLE analytics.evaluations IS
    'One immutable atomic envelope: replay key includes scope/release/configuration/window/cutoff/canonical inputs; ordered output array positions preserve ADR 0007 IDs. Hash conflicts require full-content comparison, never assumed equality. Configuration identity is host-pinned; no activation/configuration administration here.';
CREATE FUNCTION analytics.validate_evaluation() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE k JSONB := NEW.replay_key; d analytics.calculator_definitions;
BEGIN
    IF jsonb_typeof(k) IS DISTINCT FROM 'object' OR NOT k ?& ARRAY['scope','calculator','version','configuration','start','end','finalize','known','inputs']
       OR k - ARRAY['scope','calculator','version','configuration','start','end','finalize','known','inputs'] <> '{}'::jsonb
       OR metadata.is_valid_identifier(k->>'configuration') IS DISTINCT FROM true
       OR EXISTS (SELECT 1 FROM jsonb_each(k) e WHERE e.key IN ('scope','calculator','version','configuration') AND jsonb_typeof(e.value)<>'string')
       OR EXISTS (SELECT 1 FROM jsonb_each(k) e WHERE e.key IN ('start','end','finalize','known') AND jsonb_typeof(e.value)<>'number')
       OR jsonb_typeof(k->'inputs') IS DISTINCT FROM 'array' THEN RAISE EXCEPTION 'Invalid evaluation key'; END IF;
    IF (k->>'end')::bigint::numeric-(k->>'start')::bigint::numeric NOT BETWEEN 1 AND 604800000
       OR (k->>'finalize')::bigint::numeric-(k->>'end')::bigint::numeric NOT BETWEEN 0 AND 604800000
       OR (k->>'known')::bigint < (k->>'finalize')::bigint OR NEW.evaluated_at_ms < (k->>'known')::bigint
       OR jsonb_array_length(k->'inputs')>1024 THEN RAISE EXCEPTION 'Invalid evaluation bounds'; END IF;
    SELECT * INTO STRICT d FROM analytics.calculator_definitions WHERE tenant_id=NEW.tenant_id AND calculator_id=k->>'calculator' AND calculator_version=k->>'version';
    PERFORM 1 FROM metadata.assets WHERE tenant_id=NEW.tenant_id AND asset_id=k->>'scope';
    IF NOT FOUND THEN RAISE EXCEPTION 'Missing evaluation scope'; END IF;
    IF (SELECT count(*) FROM jsonb_array_elements(k->'inputs')) <> (SELECT count(DISTINCT i->>0) FROM jsonb_array_elements(k->'inputs') i) THEN RAISE EXCEPTION 'Duplicate evaluation evidence'; END IF;
    IF EXISTS (
        SELECT 1 FROM (
            SELECT value, lag(value) OVER (ORDER BY ordinal) AS previous
            FROM jsonb_array_elements(k->'inputs') WITH ORDINALITY AS inputs(value,ordinal)
        ) i LEFT JOIN history.asset_property_changes h ON h.tenant_id=NEW.tenant_id
          AND analytics.semantic_evidence_id(h.tenant_id,h.message_id,h.protocol_package_id,h.protocol_package_version,h.output_index,h.binding_id,h.binding_revision,h.event_time_basis)=i.value->>0
        LEFT JOIN metadata.signal_binding_revisions b ON b.tenant_id=h.tenant_id AND b.binding_id=h.binding_id AND b.revision=h.binding_revision
        WHERE jsonb_typeof(i.value) IS DISTINCT FROM 'array' OR jsonb_array_length(i.value)<>2
          OR jsonb_typeof(i.value->0) IS DISTINCT FROM 'string' OR jsonb_typeof(i.value->1) IS DISTINCT FROM 'number'
          OR h.tenant_id IS NULL OR h.event_at_ms<>(i.value->>1)::bigint OR h.resolution_known_at_ms>(k->>'known')::bigint
          OR h.event_at_ms<(k->>'start')::bigint OR h.event_at_ms>=(k->>'end')::bigint
          OR b.target_asset_id IS DISTINCT FROM k->>'scope' OR b.target_property_id IS DISTINCT FROM d.input_id OR b.target_property_version IS DISTINCT FROM d.input_version
          OR (i.previous IS NOT NULL AND ((i.value->>1)::bigint,(i.value->>0) COLLATE "C") <= ((i.previous->>1)::bigint,(i.previous->>0) COLLATE "C"))
    ) THEN RAISE EXCEPTION 'Invalid or noncanonical evaluation evidence'; END IF;
    IF EXISTS (
        SELECT 1 FROM jsonb_array_elements(NEW.outputs) item
        LEFT JOIN metadata.assets a ON a.tenant_id=NEW.tenant_id AND a.asset_id=item->>'asset'
        LEFT JOIN metadata.asset_type_properties ap ON ap.tenant_id=a.tenant_id AND ap.type_id=a.type_id AND ap.type_version=a.type_version
          AND ap.property_id=d.output_id AND ap.property_version=d.output_version
        LEFT JOIN metadata.property_definitions p ON p.tenant_id=ap.tenant_id AND p.property_id=ap.property_id AND p.version=ap.property_version
        WHERE p.property_id IS NULL OR jsonb_typeof(item) IS DISTINCT FROM 'object'
          OR NOT item ?& ARRAY['asset','property','version','kind','value','unit']
          OR item - ARRAY['asset','property','version','kind','value','unit'] <> '{}'::jsonb
          OR item->>'property' IS DISTINCT FROM p.property_id OR (item->>'version')::bigint IS DISTINCT FROM p.version
          OR item->>'kind' IS DISTINCT FROM 'decimal' OR item->>'unit' IS DISTINCT FROM p.canonical_unit
          OR jsonb_typeof(item->'value') IS DISTINCT FROM 'string' OR length(item->>'value')>4096
          OR (item->>'value')::numeric IN ('NaN'::numeric,'Infinity'::numeric,'-Infinity'::numeric)
    ) THEN RAISE EXCEPTION 'Invalid calculated output'; END IF;
    INSERT INTO analytics.evaluation_configurations VALUES
        (NEW.tenant_id,k->>'configuration',d.calculator_id,d.calculator_version,(k->>'end')::bigint-(k->>'start')::bigint,(k->>'finalize')::bigint-(k->>'end')::bigint,NEW.minimum_samples)
        ON CONFLICT DO NOTHING;
    PERFORM 1 FROM analytics.evaluation_configurations c WHERE c.tenant_id=NEW.tenant_id AND c.configuration_id=k->>'configuration'
        AND c.calculator_id=d.calculator_id AND c.calculator_version=d.calculator_version AND c.window_ms=(k->>'end')::bigint-(k->>'start')::bigint
        AND c.lateness_ms=(k->>'finalize')::bigint-(k->>'end')::bigint AND c.minimum_samples=NEW.minimum_samples;
    IF NOT FOUND THEN RAISE EXCEPTION 'Calculator configuration identity collision' USING ERRCODE='23505'; END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER evaluations_validate BEFORE INSERT ON analytics.evaluations FOR EACH ROW EXECUTE FUNCTION analytics.validate_evaluation();
CREATE TRIGGER evaluations_immutable BEFORE UPDATE OR DELETE ON analytics.evaluations FOR EACH ROW EXECUTE FUNCTION analytics.prevent_definition_mutation();

-- Durable output-target references prevent later asset deletion from orphaning
-- JSON evidence. Derived in the same insert transaction; not a second writer API.
CREATE TABLE analytics.evaluation_targets (
    tenant_id TEXT NOT NULL,
    evaluation_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    PRIMARY KEY(tenant_id,evaluation_id,asset_id),
    FOREIGN KEY(tenant_id,evaluation_id) REFERENCES analytics.evaluations(tenant_id,evaluation_id),
    FOREIGN KEY(tenant_id,asset_id) REFERENCES metadata.assets(tenant_id,asset_id)
);
CREATE FUNCTION analytics.retain_evaluation_targets() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO analytics.evaluation_targets
    SELECT DISTINCT NEW.tenant_id,NEW.evaluation_id,value->>'asset' FROM jsonb_array_elements(NEW.outputs);
    RETURN NEW;
END;
$$;
CREATE TRIGGER evaluations_targets AFTER INSERT ON analytics.evaluations FOR EACH ROW EXECUTE FUNCTION analytics.retain_evaluation_targets();
CREATE TRIGGER evaluation_targets_immutable BEFORE UPDATE OR DELETE ON analytics.evaluation_targets FOR EACH ROW EXECUTE FUNCTION analytics.prevent_definition_mutation();
