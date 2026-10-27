-- Bounded hot-history reads and independently delegated telemetry access.
-- No existing grant is upgraded. Deploy readers before granting telemetry.read;
-- older application versions reject this new closed vocabulary. Both small
-- identity tables take an ordinary schema lock while action checks expand.
ALTER TABLE identity.tenant_action_grants DROP CONSTRAINT ck_tenant_action_grants_action;
ALTER TABLE identity.tenant_action_grants ADD CONSTRAINT ck_tenant_action_grants_action
    CHECK (action IN ('metadata.read', 'metadata.manage', 'access.manage', 'protocol.manage', 'telemetry.read'));
ALTER TABLE identity.tenant_access_changes DROP CONSTRAINT ck_tenant_access_changes_action;
ALTER TABLE identity.tenant_access_changes ADD CONSTRAINT ck_tenant_access_changes_action
    CHECK (action IN ('metadata.read', 'metadata.manage', 'access.manage', 'protocol.manage', 'telemetry.read'));
COMMENT ON COLUMN identity.tenant_action_grants.action IS
    'Exact independently delegated action; telemetry.read grants history/provenance reads, never metadata writes or protocol admission.';

-- Find immutable binding targets without selecting current metadata. Include
-- every revision: a correction never changes the target of an older fact.
CREATE INDEX ix_signal_binding_revisions_history_target
    ON metadata.signal_binding_revisions
       (tenant_id, target_asset_id, target_property_id, target_property_version, binding_id, revision);

-- The bounded target join seeks event ranges within each exact binding. The
-- remaining full identity breaks ties between outputs and interpretations.
-- Values and payloads are not duplicated. Index construction scans existing
-- rows and briefly blocks writes; use a reviewed maintenance window on large
-- installations. This transactional migration does not rewrite source tables.
CREATE INDEX ix_asset_property_changes_binding_event
    ON history.asset_property_changes
       (tenant_id, binding_id, binding_revision, event_at_ms,
        message_id, protocol_package_id, protocol_package_version, output_index, event_time_basis);
COMMENT ON INDEX history.ix_asset_property_changes_binding_event IS
    'Exact-binding bounded event-time history access; immutable full identities provide deterministic keyset ties.';
COMMENT ON INDEX metadata.ix_signal_binding_revisions_history_target IS
    'All immutable revisions targeting one asset and exact property version; never current-binding selection.';
