-- Evidence-driven alert checkpoints, immutable transitions and transactional
-- notification intents. No delivery worker or automatic historical notifications.
-- Tenant/rule/revision identifies a lifecycle; its scope is pinned by that revision.

CREATE TABLE analytics.alert_lifecycles (
    tenant_id TEXT NOT NULL,
    rule_id TEXT NOT NULL,
    revision BIGINT NOT NULL,
    -- Deployment/creation boundary never moves on worker restart. Older facts
    -- cannot become live merely because a rule was selected before this feature.
    live_from_ms BIGINT NOT NULL DEFAULT (floor(extract(epoch FROM clock_timestamp())*1000)::bigint),
    activation_generation BIGINT NOT NULL DEFAULT 0,
    version BIGINT NOT NULL DEFAULT 0,
    phase TEXT NOT NULL DEFAULT 'inactive',
    candidate_since_ms BIGINT,
    last_event_at_ms BIGINT,
    last_turn_at TIMESTAMPTZ NOT NULL DEFAULT '-infinity',
    CONSTRAINT pk_alert_lifecycles PRIMARY KEY(tenant_id,rule_id,revision),
    CONSTRAINT fk_alert_lifecycle_revision FOREIGN KEY(tenant_id,rule_id,revision)
        REFERENCES analytics.alert_rule_revisions(tenant_id,rule_id,revision),
    CONSTRAINT ck_alert_lifecycle_counters CHECK(activation_generation BETWEEN 0 AND 4294967295 AND version>=0),
    CONSTRAINT ck_alert_lifecycle_phase CHECK(phase IN ('inactive','pending','firing','resolved')),
    CONSTRAINT ck_alert_lifecycle_candidate CHECK(
        (phase<>'pending' OR candidate_since_ms IS NOT NULL)
        AND (phase NOT IN ('inactive','resolved') OR candidate_since_ms IS NULL)
        AND (candidate_since_ms IS NULL OR (last_event_at_ms IS NOT NULL AND candidate_since_ms<=last_event_at_ms))
        AND (last_event_at_ms IS NOT NULL OR phase='inactive'))
);
-- Checkpoint identity and initial live boundary cannot be repurposed/reset.
CREATE FUNCTION analytics.protect_alert_lifecycle() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' OR ROW(NEW.tenant_id,NEW.rule_id,NEW.revision,NEW.live_from_ms)
      IS DISTINCT FROM ROW(OLD.tenant_id,OLD.rule_id,OLD.revision,OLD.live_from_ms)
    THEN RAISE EXCEPTION 'Alert lifecycle identity is immutable'; END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER alert_lifecycle_identity BEFORE UPDATE OR DELETE ON analytics.alert_lifecycles
    FOR EACH ROW EXECUTE FUNCTION analytics.protect_alert_lifecycle();
COMMENT ON FUNCTION analytics.protect_alert_lifecycle() IS 'Retains revision identity and deployment/creation live boundary while permitting transactional checkpoint advancement.';
-- Seed revision-local state without consuming evidence or changing activation.
CREATE FUNCTION analytics.initialize_alert_lifecycle() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO analytics.alert_lifecycles(tenant_id,rule_id,revision)
    VALUES(NEW.tenant_id,NEW.rule_id,NEW.revision);
    RETURN NEW;
END;
$$;
CREATE TRIGGER alert_lifecycle_initialize AFTER INSERT ON analytics.alert_rule_revisions
    FOR EACH ROW EXECUTE FUNCTION analytics.initialize_alert_lifecycle();
INSERT INTO analytics.alert_lifecycles(tenant_id,rule_id,revision)
SELECT tenant_id,rule_id,revision FROM analytics.alert_rule_revisions;

-- Anti-join receipts avoid sequence-cursor gaps when producers commit late.
-- One-shot/historical/late inputs are retained without mutating lifecycle or notifying.
CREATE TABLE analytics.alert_evaluation_receipts (
    tenant_id TEXT NOT NULL,
    rule_id TEXT NOT NULL,
    revision BIGINT NOT NULL,
    evaluation_id TEXT NOT NULL,
    activation_generation BIGINT NOT NULL,
    disposition TEXT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT pk_alert_evaluation_receipts PRIMARY KEY(tenant_id,rule_id,revision,evaluation_id),
    CONSTRAINT fk_alert_receipt_lifecycle FOREIGN KEY(tenant_id,rule_id,revision)
        REFERENCES analytics.alert_lifecycles(tenant_id,rule_id,revision),
    CONSTRAINT fk_alert_receipt_evaluation FOREIGN KEY(tenant_id,evaluation_id)
        REFERENCES analytics.evaluations(tenant_id,evaluation_id),
    CONSTRAINT ck_alert_receipt_generation CHECK(activation_generation BETWEEN 1 AND 4294967295),
    CONSTRAINT ck_alert_receipt_disposition CHECK(disposition IN ('applied','missing','historical','out_of_order'))
);
-- One immutable transition per phase change, linked to the exact consumed evidence.
CREATE TABLE analytics.alert_transitions (
    tenant_id TEXT NOT NULL,
    rule_id TEXT NOT NULL,
    revision BIGINT NOT NULL,
    version BIGINT NOT NULL,
    evaluation_id TEXT NOT NULL,
    phase TEXT NOT NULL,
    event_at_ms BIGINT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT pk_alert_transitions PRIMARY KEY(tenant_id,rule_id,revision,version),
    CONSTRAINT uq_alert_transition_evaluation UNIQUE(tenant_id,rule_id,revision,evaluation_id),
    CONSTRAINT fk_alert_transition_receipt FOREIGN KEY(tenant_id,rule_id,revision,evaluation_id)
        REFERENCES analytics.alert_evaluation_receipts(tenant_id,rule_id,revision,evaluation_id),
    CONSTRAINT ck_alert_transition_version CHECK(version>0),
    CONSTRAINT ck_alert_transition_phase CHECK(phase IN ('inactive','pending','firing','resolved'))
);
-- Immutable outbound intentions only. Delivery leases, retries and network I/O
-- belong to PR15; no process currently consumes this table for remote effects.
CREATE TABLE analytics.alert_notification_intents (
    tenant_id TEXT NOT NULL,
    rule_id TEXT NOT NULL,
    revision BIGINT NOT NULL,
    transition_version BIGINT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT pk_alert_notification_intents PRIMARY KEY(tenant_id,rule_id,revision,transition_version),
    CONSTRAINT fk_alert_notification_transition FOREIGN KEY(tenant_id,rule_id,revision,transition_version)
        REFERENCES analytics.alert_transitions(tenant_id,rule_id,revision,version)
);
CREATE TRIGGER alert_receipts_immutable BEFORE UPDATE OR DELETE ON analytics.alert_evaluation_receipts
    FOR EACH ROW EXECUTE FUNCTION analytics.prevent_definition_mutation();
CREATE TRIGGER alert_transitions_immutable BEFORE UPDATE OR DELETE ON analytics.alert_transitions
    FOR EACH ROW EXECUTE FUNCTION analytics.prevent_definition_mutation();
CREATE TRIGGER alert_notification_intents_immutable BEFORE UPDATE OR DELETE ON analytics.alert_notification_intents
    FOR EACH ROW EXECUTE FUNCTION analytics.prevent_definition_mutation();
COMMENT ON TABLE analytics.alert_lifecycles IS 'Revision/scope-pinned checkpoint. Event times and duration candidates are signed Unix milliseconds. Activation and unknown/gapped evidence break timer continuity; firing survives unknown evidence.';
COMMENT ON COLUMN analytics.alert_lifecycles.live_from_ms IS 'Immutable deployment/revision-creation boundary in Unix milliseconds. Live processing also requires the current activation receipt boundary.';
COMMENT ON COLUMN analytics.alert_lifecycles.version IS 'Monotonic phase-transition number; duration-only checkpoint changes do not increment it.';
COMMENT ON TABLE analytics.alert_evaluation_receipts IS 'Exactly-once consumption receipt per rule revision/evaluation; historical and non-increasing event ends never mutate lifecycle or create intents.';
COMMENT ON TABLE analytics.alert_transitions IS 'Immutable evidence-linked phase history. Event time is the consumed calculator window end, distinct from database receipt time.';
COMMENT ON TABLE analytics.alert_notification_intents IS 'Transactional outbox identities referencing firing/resolved transitions. No delivery, destination, acknowledgement or lease semantics in this slice.';
COMMENT ON FUNCTION analytics.initialize_alert_lifecycle() IS 'Initializes a revision-local inactive checkpoint without replay, activation or external effects.';

-- Exact selector plus anti-receipt lookup used by candidate discovery and the
-- locked processing turn. Empty/failed envelopes explicitly interrupt timers.
CREATE FUNCTION analytics.next_alert_evidence(t TEXT,c TEXT,r BIGINT)
RETURNS TABLE(evaluation_id TEXT,start_ms BIGINT,end_ms BIGINT,evaluated_ms BIGINT,value TEXT,scheduled BOOLEAN)
LANGUAGE SQL STABLE AS $$
    SELECT e.evaluation_id,(e.replay_key->>'start')::bigint,
        (e.replay_key->>'end')::bigint,e.evaluated_at_ms,
        CASE WHEN e.outcome='complete' THEN e.outputs->(d.definition->>'output_index')::int->>'value' END,
        EXISTS(SELECT 1 FROM analytics.calculator_work w WHERE w.tenant_id=e.tenant_id
            AND w.evaluation_id=e.evaluation_id AND w.status='done'
            AND w.request->>'target'=d.target_asset_id)
    FROM analytics.alert_rule_revisions d JOIN analytics.evaluations e
      ON e.tenant_id=d.tenant_id AND e.scope_asset_id=d.scope_asset_id
      AND e.calculator_id=d.calculator_id AND e.calculator_version=d.calculator_version
    WHERE d.tenant_id=t AND d.rule_id=c AND d.revision=r
      AND (e.outputs='[]'::jsonb OR (
        e.outputs->(d.definition->>'output_index')::int->>'asset'=d.target_asset_id
        AND e.outputs->(d.definition->>'output_index')::int->>'property'=d.definition->>'output_property'
        AND e.outputs->(d.definition->>'output_index')::int->'version'=d.definition->'output_version'
        AND e.outputs->(d.definition->>'output_index')::int->>'kind'='decimal'
        AND e.outputs->(d.definition->>'output_index')::int->>'unit' IS NOT DISTINCT FROM d.definition->>'threshold_unit'))
      AND NOT EXISTS(SELECT 1 FROM analytics.alert_evaluation_receipts p
        WHERE p.tenant_id=t AND p.rule_id=c AND p.revision=r AND p.evaluation_id=e.evaluation_id)
    ORDER BY (e.replay_key->>'end')::bigint,e.evaluation_id COLLATE "C" LIMIT 1
$$;
CREATE INDEX ix_alert_evaluation_selector ON analytics.evaluations
    (tenant_id,scope_asset_id,calculator_id,calculator_version,((replay_key->>'end')::bigint),evaluation_id COLLATE "C");
COMMENT ON INDEX analytics.ix_alert_evaluation_selector IS 'Release/scope-constrained ordered consumption; receipts remain the authority so late commits cannot fall behind a sequence checkpoint.';
COMMENT ON FUNCTION analytics.next_alert_evidence(TEXT,TEXT,BIGINT) IS 'At most one unconsumed exact-selector evaluation in visible event-end/ID order. No output-array expansion or sequence cursor; historical and late evidence is classified by the locked host reducer.';
-- The outbox never accepts a pending/cancellation transition, even from direct SQL.
CREATE FUNCTION analytics.validate_alert_notification_intent() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    PERFORM 1 FROM analytics.alert_transitions WHERE tenant_id=NEW.tenant_id
      AND rule_id=NEW.rule_id AND revision=NEW.revision AND version=NEW.transition_version
      AND phase IN ('firing','resolved');
    IF NOT FOUND THEN RAISE EXCEPTION 'Non-notifying alert transition' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER alert_notification_intent_validate BEFORE INSERT ON analytics.alert_notification_intents
    FOR EACH ROW EXECUTE FUNCTION analytics.validate_alert_notification_intent();
COMMENT ON FUNCTION analytics.validate_alert_notification_intent() IS 'Restricts immutable outbound intents to retained firing/resolved transitions; does not initiate delivery.';
