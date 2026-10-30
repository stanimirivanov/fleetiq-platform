-- Explicit tenant-owned configuration revisions and scheduling activation.
-- No implicit grants or activation: legacy trusted schedules import as inactive
-- revision 1, with unknown original actor represented by NULL, never fabricated.
ALTER TABLE identity.tenant_action_grants DROP CONSTRAINT ck_tenant_action_grants_action;
ALTER TABLE identity.tenant_action_grants ADD CONSTRAINT ck_tenant_action_grants_action CHECK(action IN ('metadata.read','metadata.manage','access.manage','protocol.manage','telemetry.read','analytics.manage'));
ALTER TABLE identity.tenant_access_changes DROP CONSTRAINT ck_tenant_access_changes_action;
ALTER TABLE identity.tenant_access_changes ADD CONSTRAINT ck_tenant_access_changes_action CHECK(action IN ('metadata.read','metadata.manage','access.manage','protocol.manage','telemetry.read','analytics.manage'));

-- The locked head serializes revision creation and activation with scheduler
-- enumeration/claims. An issued claim may finish; deactivation pauses unclaimed work.
CREATE TABLE analytics.calculator_configurations (
    tenant_id TEXT NOT NULL REFERENCES metadata.tenants(tenant_id),
    configuration_id TEXT NOT NULL CHECK(metadata.is_valid_identifier(configuration_id)),
    latest_revision BIGINT NOT NULL DEFAULT 0 CHECK(latest_revision BETWEEN 0 AND 4294967295),
    generation BIGINT NOT NULL DEFAULT 0 CHECK(generation BETWEEN 0 AND 4294967295),
    active_revision BIGINT,
    PRIMARY KEY(tenant_id,configuration_id)
);
CREATE TABLE analytics.calculator_configuration_revisions (
    tenant_id TEXT NOT NULL, configuration_id TEXT NOT NULL,
    revision BIGINT NOT NULL CHECK(revision BETWEEN 1 AND 4294967295),
    schedule_id TEXT NOT NULL,
    parameters JSONB NOT NULL CHECK(jsonb_typeof(parameters)='object' AND octet_length(parameters::text)<=4096),
    actor_id TEXT REFERENCES identity.actors(actor_id),
    recorded_at_ms BIGINT NOT NULL DEFAULT (floor(extract(epoch FROM clock_timestamp())*1000)::bigint),
    PRIMARY KEY(tenant_id,configuration_id,revision),
    UNIQUE(tenant_id,schedule_id),
    FOREIGN KEY(tenant_id,configuration_id) REFERENCES analytics.calculator_configurations(tenant_id,configuration_id),
    FOREIGN KEY(tenant_id,schedule_id) REFERENCES analytics.calculator_schedules(tenant_id,schedule_id)
);
ALTER TABLE analytics.calculator_configurations ADD CONSTRAINT fk_calculator_configuration_active
    FOREIGN KEY(tenant_id,configuration_id,active_revision) REFERENCES analytics.calculator_configuration_revisions(tenant_id,configuration_id,revision);
CREATE TABLE analytics.calculator_activation_changes (
    tenant_id TEXT NOT NULL, configuration_id TEXT NOT NULL,
    generation BIGINT NOT NULL CHECK(generation BETWEEN 1 AND 4294967295),
    revision BIGINT,
    actor_id TEXT NOT NULL REFERENCES identity.actors(actor_id),
    recorded_at_ms BIGINT NOT NULL DEFAULT (floor(extract(epoch FROM clock_timestamp())*1000)::bigint),
    PRIMARY KEY(tenant_id,configuration_id,generation),
    FOREIGN KEY(tenant_id,configuration_id) REFERENCES analytics.calculator_configurations(tenant_id,configuration_id),
    FOREIGN KEY(tenant_id,configuration_id,revision) REFERENCES analytics.calculator_configuration_revisions(tenant_id,configuration_id,revision)
);
CREATE TRIGGER calculator_configuration_revisions_immutable BEFORE UPDATE OR DELETE ON analytics.calculator_configuration_revisions FOR EACH ROW EXECUTE FUNCTION analytics.prevent_definition_mutation();
CREATE TRIGGER calculator_activation_changes_immutable BEFORE UPDATE OR DELETE ON analytics.calculator_activation_changes FOR EACH ROW EXECUTE FUNCTION analytics.prevent_definition_mutation();

-- Preserve original schedule/cursor/work identities while requiring explicit opt-in.
INSERT INTO analytics.calculator_configurations(tenant_id,configuration_id,latest_revision)
SELECT tenant_id,schedule_id,1 FROM analytics.calculator_schedules;
INSERT INTO analytics.calculator_configuration_revisions(tenant_id,configuration_id,revision,schedule_id,parameters)
SELECT tenant_id,schedule_id,1,schedule_id,jsonb_build_object('calculator',calculator_id,'version',calculator_version,'scope',scope_asset_id,'target',target_asset_id,'first_start',first_start_ms,'window_ms',window_ms,'lateness_ms',lateness_ms,'minimum_samples',minimum_samples) FROM analytics.calculator_schedules;

-- Recheck current authority inside the write transaction. Row-share locks order
-- grant/actor/tenant revocation against the mutation; authentication stays in host.
CREATE FUNCTION analytics.lock_configuration_authority(t TEXT,a TEXT) RETURNS VOID LANGUAGE plpgsql AS $$
BEGIN
    PERFORM 1 FROM identity.tenant_action_grants g JOIN identity.actors actor USING(actor_id) JOIN metadata.tenants tenant USING(tenant_id)
    WHERE g.tenant_id=t AND g.actor_id=a AND g.action='analytics.manage' AND actor.is_active AND tenant.is_active FOR SHARE OF g,actor,tenant;
    IF NOT FOUND THEN RAISE EXCEPTION 'Analytics authority unavailable' USING ERRCODE='42501'; END IF;
END;
$$;
CREATE FUNCTION analytics.create_configuration_revision(t TEXT,c TEXT,r BIGINT,a TEXT,p JSONB) RETURNS VOID LANGUAGE plpgsql AS $$
DECLARE head analytics.calculator_configurations; previous analytics.calculator_configuration_revisions; s TEXT;
BEGIN
    PERFORM analytics.lock_configuration_authority(t,a);
    INSERT INTO analytics.calculator_configurations(tenant_id,configuration_id) VALUES(t,c) ON CONFLICT DO NOTHING;
    SELECT * INTO STRICT head FROM analytics.calculator_configurations WHERE tenant_id=t AND configuration_id=c FOR UPDATE;
    SELECT * INTO previous FROM analytics.calculator_configuration_revisions WHERE tenant_id=t AND configuration_id=c AND revision=r;
    IF FOUND THEN
        IF previous.parameters=p AND previous.actor_id=a THEN RETURN; END IF;
        RAISE EXCEPTION 'Configuration revision conflict' USING ERRCODE='23505';
    END IF;
    IF r<>head.latest_revision+1 OR r>4294967295 THEN RAISE EXCEPTION 'Configuration revision conflict' USING ERRCODE='23505'; END IF;
    s:='configuration:'||encode(sha256(convert_to(jsonb_build_array(t,c,r)::text,'UTF8')),'hex');
    INSERT INTO analytics.calculator_schedules(tenant_id,schedule_id,calculator_id,calculator_version,configuration_id,scope_asset_id,target_asset_id,first_start_ms,window_ms,lateness_ms,minimum_samples,next_start_ms)
    SELECT t,s,d.calculator_id,d.calculator_version,s,p->>'scope',p->>'target',(p->>'first_start')::bigint,(p->>'window_ms')::bigint,(p->>'lateness_ms')::bigint,(p->>'minimum_samples')::int,(p->>'first_start')::bigint
    FROM analytics.calculator_definitions d JOIN metadata.assets target ON target.tenant_id=d.tenant_id AND target.asset_id=p->>'target'
    JOIN metadata.asset_type_properties member ON member.tenant_id=target.tenant_id AND member.type_id=target.type_id AND member.type_version=target.type_version AND member.property_id=d.output_id AND member.property_version=d.output_version
    WHERE d.tenant_id=t AND d.calculator_id=p->>'calculator' AND d.calculator_version=p->>'version';
    IF NOT FOUND THEN RAISE EXCEPTION 'Configuration prerequisites unavailable' USING ERRCODE='23503'; END IF;
    INSERT INTO analytics.calculator_configuration_revisions(tenant_id,configuration_id,revision,schedule_id,parameters,actor_id) VALUES(t,c,r,s,p,a);
    UPDATE analytics.calculator_configurations SET latest_revision=r WHERE tenant_id=t AND configuration_id=c;
END;
$$;
CREATE FUNCTION analytics.change_configuration_activation(t TEXT,c TEXT,e BIGINT,r BIGINT,a TEXT) RETURNS BIGINT LANGUAGE plpgsql AS $$
DECLARE head analytics.calculator_configurations; previous analytics.calculator_activation_changes;
BEGIN
    PERFORM analytics.lock_configuration_authority(t,a);
    SELECT * INTO head FROM analytics.calculator_configurations WHERE tenant_id=t AND configuration_id=c FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Configuration unavailable' USING ERRCODE='23503'; END IF;
    SELECT * INTO previous FROM analytics.calculator_activation_changes WHERE tenant_id=t AND configuration_id=c AND generation=e+1;
    IF FOUND AND previous.actor_id=a AND previous.revision IS NOT DISTINCT FROM r THEN RETURN e+1; END IF;
    IF head.generation<>e OR e>=4294967295 THEN RAISE EXCEPTION 'Activation generation conflict' USING ERRCODE='23505'; END IF;
    INSERT INTO analytics.calculator_activation_changes(tenant_id,configuration_id,generation,revision,actor_id) VALUES(t,c,e+1,r,a);
    UPDATE analytics.calculator_configurations SET generation=e+1,active_revision=r WHERE tenant_id=t AND configuration_id=c;
    RETURN e+1;
END;
$$;
COMMENT ON TABLE analytics.calculator_configurations IS 'Locked configuration/activation heads. Inactive by default; old pending work pauses when its revision is not selected.';
COMMENT ON TABLE analytics.calculator_configuration_revisions IS 'Immutable schedule revisions and original actor/time receipts; NULL actor only for imported legacy trusted schedules. Times are Unix milliseconds.';
COMMENT ON TABLE analytics.calculator_activation_changes IS 'Immutable optimistic activation receipts; NULL revision means inactive, not deletion. Same-actor exact retries retain original outcome.';
COMMENT ON FUNCTION analytics.lock_configuration_authority(TEXT,TEXT) IS 'Serializes current analytics.manage authority and lifecycle state with configuration writes; grants no authority by itself.';
COMMENT ON FUNCTION analytics.create_configuration_revision(TEXT,TEXT,BIGINT,TEXT,JSONB) IS 'Atomically registers compatible immutable schedule content and advances the locked revision head without activation.';
COMMENT ON FUNCTION analytics.change_configuration_activation(TEXT,TEXT,BIGINT,BIGINT,TEXT) IS 'Selects a retained revision or deactivates under optimistic generation fencing; serializes with new claims, never rewrites existing work.';
