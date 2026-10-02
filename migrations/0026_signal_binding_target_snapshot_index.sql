-- Support bounded bitemporal attribution reads by semantic target asset.
-- Signal-binding revisions remain append-only and tenant-owned. This migration
-- adds only an access path; it changes no stored meaning or write invariant.
-- Creating this ordinary index takes a table lock while it builds. FleetIQ is
-- pre-release; a future large deployed table needs a separately planned online
-- index rollout rather than applying this migration during active ingestion.

-- Candidate discovery selects revisions that targeted one asset by a recorded
-- cutoff. The tenant and target prefix avoids scanning other assets; recorded
-- time and stable binding ID support historical candidate and cursor filters.
-- The final latest-known revision is still selected through the existing
-- (tenant_id, binding_id, revision) primary key before target/effective tests.
CREATE INDEX ix_signal_binding_revisions_target_snapshot
ON metadata.signal_binding_revisions (
    tenant_id,
    target_asset_id,
    recorded_at_ms,
    binding_id
);

COMMENT ON INDEX metadata.ix_signal_binding_revisions_target_snapshot IS
    'Tenant-owned target-asset candidate discovery for bounded bitemporal binding pages.';
