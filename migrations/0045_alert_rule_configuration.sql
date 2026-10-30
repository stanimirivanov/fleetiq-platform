-- Versioned tenant alert-rule configuration only: no evaluation or delivery.
-- Independent read/manage actions grant no implicit authority. Existing small
-- grant tables take schema locks while their closed vocabulary expands.
ALTER TABLE identity.tenant_action_grants DROP CONSTRAINT ck_tenant_action_grants_action;
ALTER TABLE identity.tenant_action_grants ADD CONSTRAINT ck_tenant_action_grants_action CHECK(action IN ('metadata.read','metadata.manage','access.manage','protocol.manage','telemetry.read','analytics.manage','alerts.read','alerts.manage'));
ALTER TABLE identity.tenant_access_changes DROP CONSTRAINT ck_tenant_access_changes_action;
ALTER TABLE identity.tenant_access_changes ADD CONSTRAINT ck_tenant_access_changes_action CHECK(action IN ('metadata.read','metadata.manage','access.manage','protocol.manage','telemetry.read','analytics.manage','alerts.read','alerts.manage'));

-- Mutable heads serialize next-revision appends and optimistic activation.
CREATE TABLE analytics.alert_rules (
    tenant_id TEXT NOT NULL REFERENCES metadata.tenants(tenant_id),
    rule_id TEXT NOT NULL CHECK(metadata.is_valid_identifier(rule_id)),
    latest_revision BIGINT NOT NULL DEFAULT 0 CHECK(latest_revision BETWEEN 0 AND 4294967295),
    generation BIGINT NOT NULL DEFAULT 0 CHECK(generation BETWEEN 0 AND 4294967295),
    active_revision BIGINT,
    CONSTRAINT pk_alert_rules PRIMARY KEY(tenant_id,rule_id),
    CHECK(generation>0 OR active_revision IS NULL)
);
-- Immutable definitions retain complete selectors, numeric grammar and author.
-- Generated references prevent cross-tenant or dangling calculator/asset links.
CREATE TABLE analytics.alert_rule_revisions (
    tenant_id TEXT NOT NULL,rule_id TEXT NOT NULL,
    revision BIGINT NOT NULL CHECK(revision BETWEEN 1 AND 4294967295),
    definition JSONB NOT NULL CHECK(jsonb_typeof(definition)='object' AND octet_length(definition::text)<=4096),
    calculator_id TEXT GENERATED ALWAYS AS (definition->>'calculator') STORED NOT NULL,
    calculator_version TEXT GENERATED ALWAYS AS (definition->>'version') STORED NOT NULL,
    scope_asset_id TEXT GENERATED ALWAYS AS (definition->>'scope') STORED NOT NULL,
    target_asset_id TEXT GENERATED ALWAYS AS (definition->>'target') STORED NOT NULL,
    actor_id TEXT NOT NULL REFERENCES identity.actors(actor_id),
    recorded_at_ms BIGINT NOT NULL DEFAULT (floor(extract(epoch FROM clock_timestamp())*1000)::bigint),
    CONSTRAINT pk_alert_rule_revisions PRIMARY KEY(tenant_id,rule_id,revision),
    FOREIGN KEY(tenant_id,rule_id) REFERENCES analytics.alert_rules(tenant_id,rule_id),
    FOREIGN KEY(tenant_id,calculator_id,calculator_version) REFERENCES analytics.calculator_definitions(tenant_id,calculator_id,calculator_version),
    FOREIGN KEY(tenant_id,scope_asset_id) REFERENCES metadata.assets(tenant_id,asset_id),
    FOREIGN KEY(tenant_id,target_asset_id) REFERENCES metadata.assets(tenant_id,asset_id)
);
ALTER TABLE analytics.alert_rules ADD CONSTRAINT fk_alert_rule_active_revision FOREIGN KEY(tenant_id,rule_id,active_revision) REFERENCES analytics.alert_rule_revisions(tenant_id,rule_id,revision);
CREATE TABLE analytics.alert_rule_activation_changes (
    tenant_id TEXT NOT NULL,rule_id TEXT NOT NULL,
    generation BIGINT NOT NULL CHECK(generation BETWEEN 1 AND 4294967295),
    revision BIGINT,
    actor_id TEXT NOT NULL REFERENCES identity.actors(actor_id),
    recorded_at_ms BIGINT NOT NULL DEFAULT (floor(extract(epoch FROM clock_timestamp())*1000)::bigint),
    CONSTRAINT pk_alert_rule_activation_changes PRIMARY KEY(tenant_id,rule_id,generation),
    FOREIGN KEY(tenant_id,rule_id) REFERENCES analytics.alert_rules(tenant_id,rule_id),
    FOREIGN KEY(tenant_id,rule_id,revision) REFERENCES analytics.alert_rule_revisions(tenant_id,rule_id,revision)
);
CREATE TRIGGER alert_rule_revisions_immutable BEFORE UPDATE OR DELETE ON analytics.alert_rule_revisions FOR EACH ROW EXECUTE FUNCTION analytics.prevent_definition_mutation();
CREATE TRIGGER alert_rule_activation_changes_immutable BEFORE UPDATE OR DELETE ON analytics.alert_rule_activation_changes FOR EACH ROW EXECUTE FUNCTION analytics.prevent_definition_mutation();

-- Validate grammar and compatibility even for direct trusted SQL writers.
CREATE FUNCTION analytics.validate_alert_rule_definition() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE d JSONB:=NEW.definition;n NUMERIC;keys TEXT[]:=ARRAY['kind','calculator','version','scope','target','output_property','output_version','output_index','comparison','threshold','threshold_unit','hold_ms','resolution','resolve_after_ms'];
BEGIN
    IF NOT d ?& keys OR d-keys<>'{}'::jsonb OR EXISTS(SELECT 1 FROM jsonb_each(d) e WHERE
      (e.key IN ('kind','calculator','version','scope','target','output_property','comparison','threshold','resolution') AND jsonb_typeof(e.value)<>'string') OR
      (e.key IN ('output_version','output_index','hold_ms','resolve_after_ms') AND (jsonb_typeof(e.value)<>'number' OR e.value::text !~ '^[0-9]+$')) OR
      (e.key='threshold_unit' AND jsonb_typeof(e.value) NOT IN ('string','null'))) THEN RAISE EXCEPTION 'Invalid alert rule shape' USING ERRCODE='23514'; END IF;
    n:=(d->>'threshold')::numeric;
    IF d->>'kind'<>'numeric-threshold-v1' OR d->>'comparison' NOT IN ('gt','gte','lt','lte') OR d->>'resolution'<>'resolve_on_false'
      OR length(d->>'threshold')>256 OR n IN ('NaN'::numeric,'Infinity'::numeric,'-Infinity'::numeric) OR abs(n)>='1e128'::numeric OR scale(trim_scale(n))>64
      OR (d->>'hold_ms')::bigint NOT BETWEEN 0 AND 604800000 OR (d->>'resolve_after_ms')::bigint NOT BETWEEN 0 AND 604800000 THEN RAISE EXCEPTION 'Invalid alert rule grammar' USING ERRCODE='23514'; END IF;
    IF (d->>'output_index')::bigint<>0 OR NOT EXISTS(
      SELECT 1 FROM analytics.calculator_definitions c JOIN metadata.property_definitions p ON p.tenant_id=c.tenant_id AND p.property_id=c.output_id AND p.version=c.output_version
      JOIN metadata.assets a ON a.tenant_id=c.tenant_id AND a.asset_id=d->>'target'
      JOIN metadata.asset_type_properties member ON member.tenant_id=a.tenant_id AND member.type_id=a.type_id AND member.type_version=a.type_version AND member.property_id=p.property_id AND member.property_version=p.version
      WHERE c.tenant_id=NEW.tenant_id AND c.calculator_id=d->>'calculator' AND c.calculator_version=d->>'version'
        AND c.implementation IN ('window-mean-v1','window-mad-v1') AND c.output_id=d->>'output_property' AND c.output_version=(d->>'output_version')::bigint
        AND p.value_kind='decimal' AND p.canonical_unit IS NOT DISTINCT FROM d->>'threshold_unit') THEN RAISE EXCEPTION 'Unsupported alert output selection' USING ERRCODE='23503'; END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER alert_rule_definition_validate BEFORE INSERT ON analytics.alert_rule_revisions FOR EACH ROW EXECUTE FUNCTION analytics.validate_alert_rule_definition();

-- Same transactional current-authority fence as calculator configuration.
CREATE FUNCTION analytics.lock_alert_rule_authority(t TEXT,a TEXT) RETURNS VOID LANGUAGE plpgsql AS $$
BEGIN
    PERFORM 1 FROM identity.tenant_action_grants g JOIN identity.actors actor USING(actor_id) JOIN metadata.tenants tenant USING(tenant_id)
      WHERE g.tenant_id=t AND g.actor_id=a AND g.action='alerts.manage' AND actor.is_active AND tenant.is_active FOR SHARE OF g,actor,tenant;
    IF NOT FOUND THEN RAISE EXCEPTION 'Alert rule authority unavailable' USING ERRCODE='42501'; END IF;
END;
$$;
CREATE FUNCTION analytics.create_alert_rule_revision(t TEXT,c TEXT,r BIGINT,a TEXT,p JSONB) RETURNS VOID LANGUAGE plpgsql AS $$
DECLARE head analytics.alert_rules;previous analytics.alert_rule_revisions;
BEGIN
    PERFORM analytics.lock_alert_rule_authority(t,a);
    INSERT INTO analytics.alert_rules(tenant_id,rule_id) VALUES(t,c) ON CONFLICT DO NOTHING;
    SELECT * INTO STRICT head FROM analytics.alert_rules WHERE tenant_id=t AND rule_id=c FOR UPDATE;
    SELECT * INTO previous FROM analytics.alert_rule_revisions WHERE tenant_id=t AND rule_id=c AND revision=r;
    IF FOUND THEN
      IF previous.definition=p AND previous.actor_id=a THEN RETURN; END IF;
      RAISE EXCEPTION 'Alert rule revision conflict' USING ERRCODE='23505';
    END IF;
    IF r<>head.latest_revision+1 OR r>4294967295 THEN RAISE EXCEPTION 'Alert rule revision conflict' USING ERRCODE='23505'; END IF;
    INSERT INTO analytics.alert_rule_revisions(tenant_id,rule_id,revision,definition,actor_id) VALUES(t,c,r,p,a);
    UPDATE analytics.alert_rules SET latest_revision=r WHERE tenant_id=t AND rule_id=c;
END;
$$;
CREATE FUNCTION analytics.change_alert_rule_activation(t TEXT,c TEXT,e BIGINT,r BIGINT,a TEXT) RETURNS BIGINT LANGUAGE plpgsql AS $$
DECLARE head analytics.alert_rules;previous analytics.alert_rule_activation_changes;
BEGIN
    PERFORM analytics.lock_alert_rule_authority(t,a);
    SELECT * INTO head FROM analytics.alert_rules WHERE tenant_id=t AND rule_id=c FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Alert rule unavailable' USING ERRCODE='23503'; END IF;
    SELECT * INTO previous FROM analytics.alert_rule_activation_changes WHERE tenant_id=t AND rule_id=c AND generation=e+1;
    IF FOUND AND previous.actor_id=a AND previous.revision IS NOT DISTINCT FROM r THEN RETURN e+1; END IF;
    IF head.generation<>e OR e>=4294967295 THEN RAISE EXCEPTION 'Alert activation conflict' USING ERRCODE='23505'; END IF;
    INSERT INTO analytics.alert_rule_activation_changes(tenant_id,rule_id,generation,revision,actor_id) VALUES(t,c,e+1,r,a);
    UPDATE analytics.alert_rules SET generation=e+1,active_revision=r WHERE tenant_id=t AND rule_id=c;
    RETURN e+1;
END;
$$;
COMMENT ON TABLE analytics.alert_rules IS 'Versioned rule/activation heads, inactive by default. No alert worker, transitions or notification delivery.';
COMMENT ON TABLE analytics.alert_rule_revisions IS 'Immutable numeric-threshold-v1 selectors, exact canonical-unit decimal predicate, event-time hold/resolve delays and original actor/time receipts. Times are Unix milliseconds.';
COMMENT ON TABLE analytics.alert_rule_activation_changes IS 'Immutable optimistic selection receipts; null revision deactivates, exact same-author retries never restore old state.';
COMMENT ON FUNCTION analytics.validate_alert_rule_definition() IS 'Closed bounded grammar and admitted exact ordinal/property/target/unit validation; no dynamic expression execution.';
COMMENT ON FUNCTION analytics.lock_alert_rule_authority(TEXT,TEXT) IS 'Locks current alerts.manage grant and active lifecycle state within the mutation transaction; no privilege elevation.';
COMMENT ON FUNCTION analytics.create_alert_rule_revision(TEXT,TEXT,BIGINT,TEXT,JSONB) IS 'Atomic next-revision creation with original same-author retry; never implicitly activates.';
COMMENT ON FUNCTION analytics.change_alert_rule_activation(TEXT,TEXT,BIGINT,BIGINT,TEXT) IS 'Optimistic selection/deactivation with retained exact command receipts; this slice does not execute rules.';
