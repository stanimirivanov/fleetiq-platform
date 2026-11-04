-- Tenant-first seeks keep diagnostic prefix sampling independent of other tenants.
-- Index builds scan retained tables and hold ordinary index-build locks; qualify
-- migration time against installed data. No lifecycle/work row is rewritten.
CREATE INDEX ix_replay_diagnostic_prefix ON ingestion.replay_work(tenant_id,message_id) WHERE status<>'completed';
CREATE INDEX ix_calculator_diagnostic_prefix ON analytics.calculator_work(tenant_id,evaluation_id) WHERE status<>'done';
CREATE INDEX ix_alert_diagnostic_prefix ON analytics.alert_rules(tenant_id,rule_id) WHERE active_revision IS NOT NULL;
CREATE INDEX ix_notification_diagnostic_prefix ON analytics.alert_notification_delivery(tenant_id,destination,rule_id,revision,firing_version);

-- Independent diagnostics authority; existing grants and immutable audit rows remain unchanged.
-- Replace action checks last, so grant/audit locks do not span the large index builds.
-- Existing rows and grants remain unchanged.
ALTER TABLE identity.tenant_action_grants DROP CONSTRAINT ck_tenant_action_grants_action;
ALTER TABLE identity.tenant_action_grants ADD CONSTRAINT ck_tenant_action_grants_action
    CHECK(action IN ('metadata.read','metadata.manage','access.manage','protocol.manage','telemetry.read','analytics.manage','alerts.read','alerts.manage','alerts.acknowledge','operations.read'));
ALTER TABLE identity.tenant_access_changes DROP CONSTRAINT ck_tenant_access_changes_action;
ALTER TABLE identity.tenant_access_changes ADD CONSTRAINT ck_tenant_access_changes_action
    CHECK(action IN ('metadata.read','metadata.manage','access.manage','protocol.manage','telemetry.read','analytics.manage','alerts.read','alerts.manage','alerts.acknowledge','operations.read'));
