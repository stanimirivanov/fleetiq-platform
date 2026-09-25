-- ============================================================================
-- FleetIQ tenant access audit query support
-- ============================================================================
--
-- Adds the access path used to read one tenant's immutable administration
-- history in deterministic newest-first pages. Equal recorded times are ordered
-- by tenant-scoped change identity, which also provides the stable second half
-- of the continuation cursor.
--
-- The index is additive and contains no new data. On a populated production
-- table, creating it takes a write-blocking table lock because migrations run
-- transactionally without CONCURRENTLY. Deployment planning must measure that
-- lock before this migration is applied to a large audit history.
-- ============================================================================


-- Supports tenant equality followed by descending keyset pagination over the
-- exact `(recorded_at_ms, change_id)` order used by the repository query.
CREATE INDEX ix_tenant_access_changes_tenant_recorded_change
ON identity.tenant_access_changes
    (tenant_id, recorded_at_ms DESC, change_id DESC);


COMMENT ON INDEX identity.ix_tenant_access_changes_tenant_recorded_change IS
    'Supports deterministic newest-first keyset pagination of one tenant access audit history.';
