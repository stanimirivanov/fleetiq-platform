-- ============================================================================
-- FleetIQ actor lifecycle audit query support
-- ============================================================================
--
-- Adds the access path used to read immutable actor lifecycle history in
-- deterministic newest-first pages. Equal recorded times are ordered by the
-- platform-scoped change identity, which is the stable second cursor component.
--
-- The index is additive and contains no new data. On a populated production
-- table, creating it takes a write-blocking table lock because migrations run
-- transactionally without CONCURRENTLY. Deployment planning must measure that
-- lock before this migration is applied to a large audit history.
-- ============================================================================


-- Supports descending keyset pagination over the exact
-- `(recorded_at_ms, change_id)` order used by the repository query.
CREATE INDEX ix_actor_lifecycle_changes_recorded_change
ON identity.actor_lifecycle_changes
    (recorded_at_ms DESC, change_id DESC);


COMMENT ON INDEX identity.ix_actor_lifecycle_changes_recorded_change IS
    'Supports deterministic newest-first keyset pagination of actor lifecycle audit history.';
