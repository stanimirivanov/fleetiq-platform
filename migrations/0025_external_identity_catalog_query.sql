-- ============================================================================
-- FleetIQ current external-identity catalogue query support
-- ============================================================================
--
-- Adds the access path used to read one actor's current external identities in
-- deterministic authority-and-subject order. Actor scope is the leading key so
-- a query cannot cross actor boundaries while following a continuation cursor.
--
-- The replacement index contains the actor-only foreign-key access path as its
-- leading column. After the composite index is built, the narrower predecessor
-- is removed to avoid duplicate write and storage cost. On a populated table,
-- creating the replacement takes a write-blocking table lock because FleetIQ
-- migrations run transactionally without CONCURRENTLY. Deployment planning
-- must measure that lock before applying this migration to a large identity
-- mapping table.
-- ============================================================================


-- Supports actor-scoped keyset pagination over the exact
-- `(authority_id, subject)` order used by the repository query. It also serves
-- actor foreign-key joins and lifecycle checks through its leading column.
CREATE INDEX ix_external_actor_identities_actor_identity
ON identity.external_actor_identities
    (actor_id, authority_id, subject);

DROP INDEX identity.ix_external_actor_identities_actor;


COMMENT ON INDEX identity.ix_external_actor_identities_actor_identity IS
    'Supports actor-scoped keyset pagination of current external identities and actor foreign-key access.';
