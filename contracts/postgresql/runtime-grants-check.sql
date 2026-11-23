-- Synthetic source lineage for the two invoker-trigger handoffs. The test owns
-- the isolated schema and disposes it. It does not commission a demo tenant.
SET LOCAL ROLE grant_test_serve;
INSERT INTO metadata.tenants (tenant_id) VALUES ('grant-contract');
INSERT INTO metadata.property_definitions
    (tenant_id, property_id, version, display_name, value_kind, canonical_unit)
VALUES ('grant-contract', 'electrical.voltage', 1, 'Voltage', 'integer', 'volt');
INSERT INTO metadata.asset_type_definitions
    (tenant_id, type_id, version, display_name, property_count)
VALUES ('grant-contract', 'gateway', 1, 'Gateway', 0),
       ('grant-contract', 'system', 1, 'System', 1);
INSERT INTO metadata.asset_type_properties
    (tenant_id, type_id, type_version, property_id, property_version, property_ordinal)
VALUES ('grant-contract', 'system', 1, 'electrical.voltage', 1, 1);
INSERT INTO metadata.assets (tenant_id, asset_id, type_id, type_version, display_name)
VALUES ('grant-contract', 'gateway-1', 'gateway', 1, 'Gateway'),
       ('grant-contract', 'system-1', 'system', 1, 'System');
INSERT INTO metadata.communication_devices (tenant_id, device_asset_id)
VALUES ('grant-contract', 'gateway-1');
INSERT INTO metadata.device_endpoints
    (tenant_id, device_asset_id, endpoint_id, endpoint_kind_id, display_name, endpoint_ordinal)
VALUES ('grant-contract', 'gateway-1', 'can', 'can', 'CAN', 1);

INSERT INTO metadata.signal_bindings (tenant_id, binding_id, latest_revision)
VALUES ('grant-contract', 'voltage', 1);
INSERT INTO metadata.signal_binding_revisions
    (tenant_id, binding_id, revision, recorded_at_ms,
     source_device_asset_id, source_endpoint_id, source_signal_id,
     target_asset_id, target_property_id, target_property_version, effective_start_ms)
VALUES ('grant-contract', 'voltage', 1, 100, 'gateway-1', 'can', 'bus.voltage',
        'system-1', 'electrical.voltage', 1, 0);
RESET ROLE;

SET LOCAL ROLE grant_test_bootstrap_platform_operator;
INSERT INTO identity.platform_bootstrap (singleton, actor_id, authority_id, subject, recorded_at_ms)
VALUES (true, 'grant-operator', 'qualification', 'synthetic', 1);
INSERT INTO identity.actors (actor_id) VALUES ('grant-operator');
INSERT INTO identity.external_actor_identities (authority_id, subject, actor_id)
VALUES ('qualification', 'synthetic', 'grant-operator');
INSERT INTO identity.platform_action_grants (actor_id, action)
VALUES ('grant-operator', 'platform.access.manage'), ('grant-operator', 'identity.manage'),
       ('grant-operator', 'tenant.provision'), ('grant-operator', 'tenant.lifecycle.manage');
RESET ROLE;

SET LOCAL ROLE grant_test_ingest_tcp;
DO $$ BEGIN
    IF NOT EXISTS (SELECT tenant_id FROM metadata.tenants WHERE tenant_id = 'grant-contract' FOR SHARE) THEN
        RAISE EXCEPTION 'Ingestion cannot lock its active tenant';
    END IF;
    BEGIN UPDATE metadata.tenants SET is_active = false WHERE tenant_id = 'grant-contract';
        RAISE EXCEPTION 'Ingestion may deactivate tenant';
    EXCEPTION WHEN insufficient_privilege THEN NULL; END;
    BEGIN UPDATE metadata.tenants SET runtime_lock_guard = false WHERE tenant_id = 'grant-contract';
        RAISE EXCEPTION 'Ingestion may write lock marker';
    EXCEPTION WHEN SQLSTATE '428C9' THEN NULL; END;
    BEGIN UPDATE metadata.tenants SET tenant_id = 'renamed' WHERE tenant_id = 'grant-contract';
        RAISE EXCEPTION 'Ingestion may rename tenant';
    EXCEPTION WHEN insufficient_privilege THEN NULL; END;
    BEGIN SELECT projection.project_next();
        RAISE EXCEPTION 'Ingestion may run projection';
    EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
INSERT INTO ingestion.accepted_messages
    (tenant_id, message_id, device_asset_id, protocol_package_id, protocol_package_version, received_at_ms, payload)
VALUES ('grant-contract', 'message-1', 'gateway-1', 'vendor.protocol', '1', 1100, '\x01');
RESET ROLE;

SET LOCAL ROLE grant_test_replay_worker;
INSERT INTO ingestion.decoded_message_results
    (tenant_id, message_id, protocol_package_id, protocol_package_version, observation_count)
VALUES ('grant-contract', 'message-1', 'vendor.protocol', '1', 1);
INSERT INTO ingestion.decoded_observations
    (tenant_id, message_id, protocol_package_id, protocol_package_version,
     output_index, endpoint_id, signal_id, device_recorded_at_ms, value_kind, value_integer, source_unit, quality)
VALUES ('grant-contract', 'message-1', 'vendor.protocol', '1', 0, 'can', 'bus.voltage', 1000, 'integer', 48, 'volt', 'good');
RESET ROLE;

SET LOCAL ROLE grant_test_semantic_worker;
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM history.semantic_work WHERE tenant_id = 'grant-contract' AND message_id = 'message-1' FOR UPDATE) THEN
        RAISE EXCEPTION 'Replay did not hand off to semantic work';
    END IF;
END $$;
RESET ROLE;

SET LOCAL ROLE grant_test_semantic_worker;
INSERT INTO history.asset_property_changes
    (tenant_id, message_id, protocol_package_id, protocol_package_version,
     output_index, binding_id, binding_revision, event_time_basis, event_at_ms,
     resolution_effective_at_ms, resolution_known_at_ms)
VALUES ('grant-contract', 'message-1', 'vendor.protocol', '1', 0, 'voltage', 1,
        'device_recorded', 1000, 1000, 1200);
RESET ROLE;

SET LOCAL ROLE grant_test_project_current;
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM projection.project_next() WHERE applied_count = 1) THEN
        RAISE EXCEPTION 'Semantic append did not hand off to current projection';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM projection.current_properties WHERE tenant_id = 'grant-contract'
        AND asset_id = 'system-1' AND property_id = 'electrical.voltage') THEN
        RAISE EXCEPTION 'Current projection did not advance';
    END IF;
END $$;
RESET ROLE;

-- Use a synthetic policy and calculator definition owned by the fixture. The
-- archive and analytics workers still exercise their own narrower grants.
INSERT INTO history.lifecycle_policies (tenant_id, revision, actor_id, hot_days, archive_days, max_jobs, max_bytes)
VALUES ('grant-contract', 1, 'grant-operator', 1, 2, 4, 8388608);
INSERT INTO identity.tenant_action_grants VALUES ('grant-contract', 'grant-operator', 'alerts.manage');
INSERT INTO metadata.property_definitions
    (tenant_id, property_id, version, display_name, value_kind, canonical_unit)
VALUES ('grant-contract', 'mean.voltage', 1, 'Mean', 'decimal', 'volt');
INSERT INTO metadata.asset_type_definitions
    (tenant_id, type_id, version, display_name, property_count)
VALUES ('grant-contract', 'calculated', 1, 'Calculated', 1);
INSERT INTO metadata.asset_type_properties
    (tenant_id, type_id, type_version, property_id, property_version, property_ordinal)
VALUES ('grant-contract', 'calculated', 1, 'mean.voltage', 1, 1);
INSERT INTO metadata.assets (tenant_id, asset_id, type_id, type_version, display_name)
VALUES ('grant-contract', 'result', 'calculated', 1, 'Result');
INSERT INTO analytics.calculator_definitions
VALUES ('grant-contract', 'mean', '1', 'window-mean-v1', 'electrical.voltage', 1, 'mean.voltage', 1);

SET LOCAL ROLE grant_test_serve;
SELECT analytics.create_alert_rule_revision('grant-contract','high',1,'grant-operator',
    '{"kind":"numeric-threshold-v1","calculator":"mean","version":"1","scope":"system-1","target":"result","output_property":"mean.voltage","output_version":1,"output_index":0,"comparison":"gt","threshold":"50","threshold_unit":"volt","hold_ms":0,"resolution":"resolve_on_false","resolve_after_ms":0}') AS synthetic_rule \gset
SELECT analytics.change_alert_rule_activation('grant-contract','high',0,1,'grant-operator') AS synthetic_generation \gset
RESET ROLE;

SET LOCAL ROLE grant_test_archive_worker;
SELECT tenant_id FROM metadata.tenants WHERE tenant_id = 'grant-contract' FOR UPDATE \gset
SELECT history.export_slots('grant-contract') AS slots, history.export_bytes('grant-contract') AS bytes \gset
INSERT INTO history.work_reservations (tenant_id, policy_revision, reserved_bytes)
VALUES ('grant-contract', 1, 2097152) RETURNING reservation_id \gset
INSERT INTO history.archive_jobs (tenant_id) VALUES ('grant-contract') RETURNING job_id \gset
INSERT INTO history.archive_members
    (tenant_id, job_id, message_id, protocol_package_id, protocol_package_version, output_index,
     binding_id, binding_revision, event_time_basis)
SELECT tenant_id, :job_id, message_id, protocol_package_id, protocol_package_version, output_index,
       binding_id, binding_revision, event_time_basis
FROM history.asset_property_changes WHERE tenant_id = 'grant-contract';
UPDATE history.archive_jobs SET generation = 1, lease_until = clock_timestamp() + interval '30 seconds',
    reservation_id = :reservation_id WHERE tenant_id = 'grant-contract' AND job_id = :job_id;
UPDATE history.archive_jobs SET object_bytes = 1, object_sha256 = sha256('\x01')
    WHERE tenant_id = 'grant-contract' AND job_id = :job_id;
DELETE FROM history.work_reservations WHERE tenant_id = 'grant-contract' AND reservation_id = :reservation_id;
RESET ROLE;

SET LOCAL ROLE grant_test_analytics_worker;
WITH evidence AS (
    SELECT jsonb_build_object('scope', 'system-1', 'calculator', 'mean', 'version', '1', 'configuration', 'c',
        'start', recorded_at_ms, 'end', recorded_at_ms + 1000, 'finalize', recorded_at_ms + 1000,
        'known', recorded_at_ms + 1000, 'inputs', '[]'::jsonb) AS key
    FROM analytics.alert_rule_activation_changes WHERE tenant_id = 'grant-contract' AND rule_id = 'high' AND generation = 1
)
INSERT INTO analytics.evaluations
    (tenant_id, evaluation_id, replay_key, key_digest, evaluated_at_ms, outcome, outputs, minimum_samples)
SELECT 'grant-contract', 'evaluation', key, sha256(convert_to(key::text, 'UTF8')), (key->>'end')::bigint,
       'complete', '[{"asset":"result","property":"mean.voltage","version":1,"kind":"decimal","value":"51","unit":"volt"}]', 1
FROM evidence;
INSERT INTO analytics.alert_evaluation_receipts
    (tenant_id, rule_id, revision, evaluation_id, activation_generation, disposition)
VALUES ('grant-contract', 'high', 1, 'evaluation', 1, 'applied');
UPDATE analytics.alert_lifecycles SET activation_generation = 1, version = 1, phase = 'firing', last_event_at_ms = 1001
WHERE tenant_id = 'grant-contract' AND rule_id = 'high' AND revision = 1;
INSERT INTO analytics.alert_transitions (tenant_id, rule_id, revision, version, evaluation_id, phase, event_at_ms)
VALUES ('grant-contract', 'high', 1, 1, 'evaluation', 'firing', 1001);
INSERT INTO analytics.alert_notification_intents (tenant_id, rule_id, revision, transition_version)
VALUES ('grant-contract', 'high', 1, 1);
RESET ROLE;

SET LOCAL ROLE grant_test_notification_worker;
INSERT INTO analytics.alert_notification_delivery (destination, tenant_id, rule_id, revision, firing_version, target_version)
SELECT sha256('\x02'), tenant_id, rule_id, revision, transition_version, transition_version
FROM analytics.alert_notification_intents WHERE tenant_id = 'grant-contract';
UPDATE analytics.alert_notification_delivery SET generation = 1, attempts = 1,
    lease_until = clock_timestamp() + interval '30 seconds' WHERE tenant_id = 'grant-contract';
UPDATE analytics.alert_notification_delivery SET accepted_version = target_version, attempts = 0, lease_until = NULL
WHERE tenant_id = 'grant-contract';
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM analytics.alert_notification_delivery
        WHERE tenant_id = 'grant-contract' AND accepted_version = 1 AND generation = 1) THEN
        RAISE EXCEPTION 'Notification worker did not reach its delivery checkpoint';
    END IF;
    BEGIN UPDATE analytics.alert_lifecycles SET phase = 'inactive' WHERE tenant_id = 'grant-contract';
        RAISE EXCEPTION 'Notification worker can rewrite alert phases';
    EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
RESET ROLE;

SET CONSTRAINTS ALL IMMEDIATE;
