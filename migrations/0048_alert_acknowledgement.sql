-- Independent operator acknowledgement: immutable intent pinned to an existing
-- phase-transition version. Never resolves, silences or changes notification work.
-- Expanding closed action constraints locks the small grant/audit tables briefly;
-- no actor receives a grant automatically.
ALTER TABLE identity.tenant_action_grants DROP CONSTRAINT ck_tenant_action_grants_action;
ALTER TABLE identity.tenant_action_grants ADD CONSTRAINT ck_tenant_action_grants_action
    CHECK(action IN ('metadata.read','metadata.manage','access.manage','protocol.manage','telemetry.read','analytics.manage','alerts.read','alerts.manage','alerts.acknowledge'));
ALTER TABLE identity.tenant_access_changes DROP CONSTRAINT ck_tenant_access_changes_action;
ALTER TABLE identity.tenant_access_changes ADD CONSTRAINT ck_tenant_access_changes_action
    CHECK(action IN ('metadata.read','metadata.manage','access.manage','protocol.manage','telemetry.read','analytics.manage','alerts.read','alerts.manage','alerts.acknowledge'));

-- One first-author receipt per exact lifecycle version; later phase versions
-- require their own acknowledgement. Old receipts survive resolution and retries.
CREATE TABLE analytics.alert_acknowledgements (
    tenant_id TEXT NOT NULL,
    rule_id TEXT NOT NULL,
    revision BIGINT NOT NULL,
    version BIGINT NOT NULL,
    actor_id TEXT NOT NULL REFERENCES identity.actors(actor_id),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT pk_alert_acknowledgements PRIMARY KEY(tenant_id,rule_id,revision,version),
    CONSTRAINT fk_alert_acknowledgement_transition FOREIGN KEY(tenant_id,rule_id,revision,version)
        REFERENCES analytics.alert_transitions(tenant_id,rule_id,revision,version)
);
CREATE TRIGGER alert_acknowledgements_immutable BEFORE UPDATE OR DELETE ON analytics.alert_acknowledgements
    FOR EACH ROW EXECUTE FUNCTION analytics.prevent_definition_mutation();

-- Lock current authority first, then the checkpoint shared with the lifecycle
-- worker. This command never acquires a rule-head lock after the checkpoint.
CREATE FUNCTION analytics.acknowledge_alert(t TEXT,c TEXT,r BIGINT,v BIGINT,a TEXT)
RETURNS VOID LANGUAGE plpgsql AS $$
DECLARE head analytics.alert_lifecycles; previous analytics.alert_acknowledgements;
BEGIN
    PERFORM 1 FROM identity.tenant_action_grants g JOIN identity.actors actor USING(actor_id)
      JOIN metadata.tenants tenant USING(tenant_id)
      WHERE g.tenant_id=t AND g.actor_id=a AND g.action='alerts.acknowledge'
        AND actor.is_active AND tenant.is_active FOR SHARE OF g,actor,tenant;
    IF NOT FOUND THEN RAISE EXCEPTION 'Alert acknowledgement authority unavailable' USING ERRCODE='42501'; END IF;
    SELECT * INTO head FROM analytics.alert_lifecycles
      WHERE tenant_id=t AND rule_id=c AND revision=r FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Alert acknowledgement conflict' USING ERRCODE='23505'; END IF;
    SELECT * INTO previous FROM analytics.alert_acknowledgements
      WHERE tenant_id=t AND rule_id=c AND revision=r AND version=v;
    IF FOUND THEN
        IF previous.actor_id=a THEN RETURN; END IF;
        RAISE EXCEPTION 'Alert acknowledgement author conflict' USING ERRCODE='23505';
    END IF;
    IF v<=0 OR head.version<>v OR head.phase NOT IN ('pending','firing') THEN
        RAISE EXCEPTION 'Alert acknowledgement version or phase conflict' USING ERRCODE='23505';
    END IF;
    INSERT INTO analytics.alert_acknowledgements(tenant_id,rule_id,revision,version,actor_id)
      VALUES(t,c,r,v,a);
END;
$$;
COMMENT ON TABLE analytics.alert_acknowledgements IS 'Immutable operator intent per tenant/rule/revision/phase version; original actor and database receipt instant. Not condition resolution, silence or notification suppression.';
COMMENT ON FUNCTION analytics.acknowledge_alert(TEXT,TEXT,BIGINT,BIGINT,TEXT) IS 'Rechecks current independent alerts.acknowledge grant/lifecycle even for retries, serializes against evidence transitions, and retains the first author/time without mutating alert phase, version, timers or notification intents.';
