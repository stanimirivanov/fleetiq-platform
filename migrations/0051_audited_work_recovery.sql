-- Recovery preserves monotonic fences and original work/evidence. Existing attempt
-- counts are backfilled once; these table scans and ordinary ALTER locks require
-- reviewed maintenance timing. Old workers MUST be stopped before migration:
-- generations no longer encode a per-cycle retry budget.
ALTER TABLE ingestion.replay_work ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3);
ALTER TABLE ingestion.replay_work DISABLE TRIGGER replay_work_guard;
UPDATE ingestion.replay_work SET attempts=generation;
-- Flush deferred provenance/FK events before further ALTER statements.
SET CONSTRAINTS ALL IMMEDIATE;
ALTER TABLE ingestion.replay_work ENABLE TRIGGER replay_work_guard;
ALTER TABLE analytics.calculator_work ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3);
ALTER TABLE analytics.calculator_work DISABLE TRIGGER calculator_work_identity;
UPDATE analytics.calculator_work SET attempts=generation;
SET CONSTRAINTS ALL IMMEDIATE;
ALTER TABLE analytics.calculator_work ENABLE TRIGGER calculator_work_identity;
ALTER TABLE ingestion.replay_work DROP CONSTRAINT ck_replay_work_generation;
ALTER TABLE ingestion.replay_work ADD CONSTRAINT ck_replay_work_generation CHECK(generation>=0);
ALTER TABLE ingestion.replay_work DROP CONSTRAINT ck_replay_work_lease;
ALTER TABLE ingestion.replay_work ADD CONSTRAINT ck_replay_work_lease CHECK((attempts=0 AND lease_until_ms IS NULL) OR (attempts>0 AND lease_until_ms IS NOT NULL));
ALTER TABLE analytics.calculator_work DROP CONSTRAINT calculator_work_generation_check;
ALTER TABLE analytics.calculator_work ADD CONSTRAINT ck_calculator_work_generation CHECK(generation>=0);

CREATE INDEX ix_replay_recovery_candidates ON ingestion.replay_work(tenant_id,message_id) WHERE status='poisoned' OR (status='pending' AND attempts=3);
CREATE INDEX ix_calculator_recovery_candidates ON analytics.calculator_work(tenant_id,evaluation_id) WHERE status='exhausted' OR (status='pending' AND attempts=3);
CREATE INDEX ix_notification_recovery_candidates ON analytics.alert_notification_delivery(tenant_id,destination,rule_id,revision,firing_version) WHERE attempts=5;

CREATE TABLE analytics.work_controls (
    tenant_id TEXT NOT NULL REFERENCES metadata.tenants(tenant_id),
    kind TEXT NOT NULL CHECK(kind IN ('replay','calculator','alert_evaluation','notification')),
    generation BIGINT NOT NULL CHECK(generation>0),
    paused BOOLEAN NOT NULL,
    PRIMARY KEY(tenant_id,kind)
);
CREATE TABLE analytics.work_recovery_audit (
    tenant_id TEXT NOT NULL REFERENCES metadata.tenants(tenant_id),
    request_id TEXT NOT NULL CHECK(metadata.is_valid_identifier(request_id)),
    actor_id TEXT NOT NULL REFERENCES identity.actors(actor_id),
    kind TEXT NOT NULL CHECK(kind IN ('replay','calculator','alert_evaluation','notification')),
    operation TEXT NOT NULL CHECK(operation IN ('retry','pause','resume')),
    target JSONB NOT NULL CHECK(jsonb_typeof(target)='object' AND octet_length(target::text)<=1024),
    expected_generation BIGINT NOT NULL CHECK(expected_generation>=0 AND expected_generation<9223372036854775807),
    generation BIGINT NOT NULL CHECK(generation=expected_generation+1),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(tenant_id,request_id)
);
CREATE INDEX ix_work_recovery_proof ON analytics.work_recovery_audit(tenant_id,kind,(target->>'id'),expected_generation) WHERE operation='retry';
CREATE TRIGGER work_recovery_audit_immutable BEFORE UPDATE OR DELETE ON analytics.work_recovery_audit FOR EACH ROW EXECUTE FUNCTION analytics.prevent_definition_mutation();
CREATE FUNCTION analytics.has_work_recovery(t TEXT,k TEXT,i TEXT,g BIGINT,n BIGINT) RETURNS BOOLEAN LANGUAGE SQL STABLE AS $$
    SELECT n=g+1 AND EXISTS(SELECT 1 FROM analytics.work_recovery_audit a WHERE a.tenant_id=t AND a.kind=k AND a.operation='retry'
      AND a.target=jsonb_build_object('id',i) AND a.expected_generation=g AND a.generation=n)
$$;
CREATE OR REPLACE FUNCTION analytics.protect_calculator_work() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Calculator work is retained'; END IF;
    IF (to_jsonb(NEW)-ARRAY['generation','lease_until_ms','status','attempts']) IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['generation','lease_until_ms','status','attempts'])
       OR NEW.generation<OLD.generation THEN RAISE EXCEPTION 'Calculator work identity is immutable'; END IF;
    IF OLD.status<>'pending' OR NEW.attempts<OLD.attempts THEN
        IF OLD.status='done' OR NEW.status<>'pending' OR NEW.attempts<>0 OR NEW.lease_until_ms IS NOT NULL
           OR NOT analytics.has_work_recovery(OLD.tenant_id,'calculator',OLD.evaluation_id,OLD.generation,NEW.generation)
        THEN RAISE EXCEPTION 'Audited recovery required'; END IF;
    END IF;
    RETURN NEW;
END; $$;

CREATE OR REPLACE FUNCTION ingestion.guard_replay_work() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Replay evidence cannot be deleted' USING ERRCODE='55000'; END IF;
    IF TG_OP='INSERT' THEN
        IF NEW.generation<>0 OR NEW.lease_until_ms IS NOT NULL OR NEW.status<>'pending' OR NEW.next_state IS NOT NULL THEN RAISE EXCEPTION 'Replay input must start pending and unclaimed' USING ERRCODE='23514'; END IF;
        IF NOT EXISTS (SELECT 1 FROM ingestion.replay_sessions s JOIN ingestion.accepted_messages m ON m.tenant_id=s.tenant_id AND m.message_id=NEW.message_id
            WHERE s.tenant_id=NEW.tenant_id AND s.session_id=NEW.session_id AND s.current_sequence+1=NEW.sequence AND s.state=NEW.pre_state
            AND m.device_asset_id=s.device_asset_id AND m.protocol_package_id=s.protocol_package_id AND m.protocol_package_version=s.protocol_package_version AND octet_length(m.payload)<=65536)
        THEN RAISE EXCEPTION 'Invalid replay predecessor or provenance' USING ERRCODE='23514'; END IF;
    ELSE
        IF NEW.generation<OLD.generation OR ROW(NEW.tenant_id,NEW.message_id,NEW.session_id,NEW.sequence,NEW.pre_state) IS DISTINCT FROM ROW(OLD.tenant_id,OLD.message_id,OLD.session_id,OLD.sequence,OLD.pre_state)
        THEN RAISE EXCEPTION 'Replay input and terminal receipts are immutable' USING ERRCODE='55000'; END IF;
        IF OLD.status<>'pending' OR NEW.attempts<OLD.attempts THEN
            IF OLD.status='completed' OR NEW.status<>'pending' OR NEW.attempts<>0 OR NEW.lease_until_ms IS NOT NULL OR NEW.next_state IS NOT NULL
               OR NOT analytics.has_work_recovery(OLD.tenant_id,'replay',OLD.message_id,OLD.generation,NEW.generation)
            THEN RAISE EXCEPTION 'Audited recovery required'; END IF;
        END IF;
    END IF;
    RETURN NEW;
END; $$;
CREATE FUNCTION analytics.guard_work_control() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Work controls are retained'; END IF;
    IF TG_OP='UPDATE' AND (ROW(NEW.tenant_id,NEW.kind) IS DISTINCT FROM ROW(OLD.tenant_id,OLD.kind) OR NEW.generation<>OLD.generation+1)
    THEN RAISE EXCEPTION 'Work control fence cannot be replaced'; END IF;
    IF NOT EXISTS(SELECT 1 FROM analytics.work_recovery_audit a WHERE a.tenant_id=NEW.tenant_id AND a.kind=NEW.kind
      AND a.operation=CASE WHEN NEW.paused THEN 'pause' ELSE 'resume' END AND a.target='{}'::jsonb AND a.generation=NEW.generation)
    THEN RAISE EXCEPTION 'Audited control required'; END IF;
    RETURN NEW;
END; $$;
CREATE TRIGGER work_controls_audited BEFORE INSERT OR UPDATE OR DELETE ON analytics.work_controls FOR EACH ROW EXECUTE FUNCTION analytics.guard_work_control();

-- Idempotence lock is scoped to the canonical tenant/request tuple. Hash collision
-- only serializes unrelated commands; receipt identity is still the full PK.
CREATE FUNCTION analytics.recover_work(t TEXT,a TEXT,q TEXT,k TEXT,o TEXT,x JSONB,g BIGINT,execution BYTEA)
RETURNS analytics.work_recovery_audit LANGUAGE plpgsql AS $$
DECLARE previous analytics.work_recovery_audit; result analytics.work_recovery_audit;
    paused_now BOOLEAN; fence BIGINT; eligible BOOLEAN;
BEGIN
    PERFORM 1 FROM identity.tenant_action_grants grant_row JOIN identity.actors actor USING(actor_id)
      JOIN metadata.tenants tenant USING(tenant_id)
      WHERE grant_row.tenant_id=t AND grant_row.actor_id=a AND grant_row.action='operations.manage' AND actor.is_active AND tenant.is_active
      FOR SHARE OF grant_row,actor,tenant;
    IF NOT FOUND THEN RAISE EXCEPTION 'Operational authority unavailable' USING ERRCODE='42501'; END IF;
    IF NOT metadata.is_valid_identifier(q) OR k NOT IN ('replay','calculator','alert_evaluation','notification') OR o NOT IN ('retry','pause','resume')
      OR g<0 OR g>=9223372036854775807 THEN RAISE EXCEPTION 'Invalid recovery command' USING ERRCODE='23514'; END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended(jsonb_build_array(t,q)::text,0));
    SELECT * INTO previous FROM analytics.work_recovery_audit WHERE tenant_id=t AND request_id=q;
    IF FOUND THEN
        IF ROW(previous.actor_id,previous.kind,previous.operation,previous.target,previous.expected_generation) IS DISTINCT FROM ROW(a,k,o,x,g)
        THEN RAISE EXCEPTION 'Recovery identity collision' USING ERRCODE='23505'; END IF;
        RETURN previous;
    END IF;
    IF o<>'retry' THEN
        IF x<>'{}'::jsonb THEN RAISE EXCEPTION 'Invalid control target' USING ERRCODE='23514'; END IF;
        -- Same-kind controls serialize even before the first retained control row.
        PERFORM pg_advisory_xact_lock(hashtextextended(jsonb_build_array('control',t,k)::text,0));
        SELECT generation,paused INTO fence,paused_now FROM analytics.work_controls WHERE tenant_id=t AND kind=k FOR UPDATE;
        IF NOT FOUND THEN fence:=0; paused_now:=false; END IF;
        IF fence<>g OR paused_now=(o='pause') THEN RAISE EXCEPTION 'Control conflict' USING ERRCODE='23505'; END IF;
    ELSIF k IN ('replay','calculator') THEN
        IF x IS DISTINCT FROM jsonb_build_object('id',x->>'id') OR NOT metadata.is_valid_identifier(x->>'id')
        THEN RAISE EXCEPTION 'Invalid work identity' USING ERRCODE='23514'; END IF;
        IF k='replay' THEN
            -- Match completion's session-before-work lock order.
            PERFORM 1 FROM ingestion.replay_sessions s JOIN ingestion.replay_work w USING(tenant_id,session_id)
              WHERE w.tenant_id=t AND w.message_id=x->>'id' FOR UPDATE OF s;
            SELECT w.generation,(w.status='poisoned' OR (w.status='pending' AND w.attempts=3))
              AND (w.lease_until_ms IS NULL OR w.lease_until_ms<=floor(extract(epoch FROM clock_timestamp())*1000)::bigint)
              AND w.generation<=9223372036854775807-4 AND s.current_sequence+1=w.sequence AND s.execution_sha256=execution AND s.execution_policy=1 AND s.admission_policy=1
              INTO fence,eligible FROM ingestion.replay_work w JOIN ingestion.replay_sessions s USING(tenant_id,session_id)
              WHERE w.tenant_id=t AND w.message_id=x->>'id' FOR UPDATE OF w,s;
        ELSE
            SELECT w.generation,(w.status='exhausted' OR (w.status='pending' AND w.attempts=3))
              AND (w.lease_until_ms IS NULL OR w.lease_until_ms<=floor(extract(epoch FROM clock_timestamp())*1000)::bigint)
              AND EXISTS(SELECT 1 FROM analytics.calculator_configuration_revisions r JOIN analytics.calculator_configurations c
                ON (c.tenant_id,c.configuration_id,c.active_revision)=(r.tenant_id,r.configuration_id,r.revision)
                WHERE r.tenant_id=w.tenant_id AND r.schedule_id=w.schedule_id) AND w.generation<=2147483647-4
              INTO fence,eligible FROM analytics.calculator_work w WHERE w.tenant_id=t AND w.evaluation_id=x->>'id' FOR UPDATE;
        END IF;
        IF NOT FOUND OR fence<>g OR eligible IS DISTINCT FROM true THEN RAISE EXCEPTION 'Work conflict' USING ERRCODE='23505'; END IF;
    ELSIF k='notification' THEN
        IF x IS DISTINCT FROM jsonb_build_object('destination',x->>'destination','rule_id',x->>'rule_id','revision',x->>'revision','firing_version',x->>'firing_version') OR x->>'destination' !~ '^[a-f0-9]{64}$' OR NOT metadata.is_valid_identifier(x->>'rule_id')
          OR x->>'revision' !~ '^[1-9][0-9]{0,9}$' OR x->>'firing_version' !~ '^[1-9][0-9]{0,18}$'
        THEN RAISE EXCEPTION 'Invalid notification identity' USING ERRCODE='23514'; END IF;
        SELECT d.generation,d.generation<=9223372036854775807-6 AND d.attempts=5 AND (d.lease_until IS NULL OR d.lease_until<=clock_timestamp())
          AND (d.accepted_version<d.target_version OR d.resolved_until IS NULL OR d.resolved_until>clock_timestamp())
          AND NOT COALESCE((SELECT min(i.transition_version)>d.target_version FROM analytics.alert_notification_intents i
            WHERE (i.tenant_id,i.rule_id,i.revision)=(d.tenant_id,d.rule_id,d.revision) AND i.transition_version>d.firing_version),false)
          INTO fence,eligible FROM analytics.alert_notification_delivery d WHERE d.tenant_id=t AND d.destination=decode(x->>'destination','hex')
          AND d.rule_id=x->>'rule_id' AND d.revision=(x->>'revision')::bigint AND d.firing_version=(x->>'firing_version')::bigint FOR UPDATE;
        IF NOT FOUND OR fence<>g OR eligible IS DISTINCT FROM true THEN RAISE EXCEPTION 'Delivery conflict' USING ERRCODE='23505'; END IF;
    ELSE RAISE EXCEPTION 'No failed alert lease exists' USING ERRCODE='23505';
    END IF;
    INSERT INTO analytics.work_recovery_audit(tenant_id,request_id,actor_id,kind,operation,target,expected_generation,generation)
      VALUES(t,q,a,k,o,x,g,g+1) RETURNING * INTO result;
    IF o<>'retry' THEN
        INSERT INTO analytics.work_controls VALUES(t,k,g+1,o='pause') ON CONFLICT(tenant_id,kind)
          DO UPDATE SET generation=EXCLUDED.generation,paused=EXCLUDED.paused;
    ELSIF k='replay' THEN
        UPDATE ingestion.replay_work SET generation=g+1,attempts=0,lease_until_ms=NULL,status='pending' WHERE tenant_id=t AND message_id=x->>'id';
    ELSIF k='calculator' THEN
        UPDATE analytics.calculator_work SET generation=(g+1)::int,attempts=0,lease_until_ms=NULL,status='pending' WHERE tenant_id=t AND evaluation_id=x->>'id';
    ELSE
        UPDATE analytics.alert_notification_delivery SET generation=g+1,attempts=0,lease_until=NULL,next_attempt_at=clock_timestamp()
          WHERE tenant_id=t AND destination=decode(x->>'destination','hex') AND rule_id=x->>'rule_id'
          AND revision=(x->>'revision')::bigint AND firing_version=(x->>'firing_version')::bigint;
    END IF;
    RETURN result;
END; $$;
COMMENT ON TABLE analytics.work_recovery_audit IS 'Immutable explicit operator recovery receipts; exact retries never reapply effects. No source payload, URL or prepared-input mutation.';
COMMENT ON TABLE analytics.work_controls IS 'Absent means generation zero and unpaused. Pause gates new observed claims/seeding, not ingress, in-flight completion, drain, rule resolution or router delivery.';
COMMENT ON COLUMN ingestion.replay_work.attempts IS 'Per-cycle attempt budget, independent of monotonic generation after audited recovery. Old binaries must not run.';
COMMENT ON COLUMN analytics.calculator_work.attempts IS 'Three attempts per cycle; audited retry advances the generation without replacing original request/preparation.';

ALTER TABLE identity.tenant_action_grants DROP CONSTRAINT ck_tenant_action_grants_action;
ALTER TABLE identity.tenant_action_grants ADD CONSTRAINT ck_tenant_action_grants_action
    CHECK(action IN ('metadata.read','metadata.manage','access.manage','protocol.manage','telemetry.read','analytics.manage','alerts.read','alerts.manage','alerts.acknowledge','operations.read','operations.manage'));
ALTER TABLE identity.tenant_access_changes DROP CONSTRAINT ck_tenant_access_changes_action;
ALTER TABLE identity.tenant_access_changes ADD CONSTRAINT ck_tenant_access_changes_action
    CHECK(action IN ('metadata.read','metadata.manage','access.manage','protocol.manage','telemetry.read','analytics.manage','alerts.read','alerts.manage','alerts.acknowledge','operations.read','operations.manage'));
