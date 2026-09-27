-- ============================================================================
-- FleetIQ actor lifecycle administration
-- ============================================================================
--
-- Adds reversible activation state to stable internal actors and immutable
-- evidence for authorized lifecycle commands. Deactivation preserves actor
-- identity, grants, external mappings, and historical foreign keys while
-- preventing new authentication and policy decisions from using the actor.
--
-- Operational note: adding the NOT NULL column with a constant default is a
-- short catalogue operation on PostgreSQL 18. The audit table and indexes do
-- not rewrite existing identity data.
-- ============================================================================


-- ============================================================================
-- Current actor lifecycle state
-- ============================================================================

ALTER TABLE identity.actors
ADD COLUMN is_active BOOLEAN NOT NULL DEFAULT TRUE;

COMMENT ON COLUMN identity.actors.is_active IS
    'Whether registered external identities may resolve to this actor and current grants may authorize new operations.';


-- ============================================================================
-- Immutable actor lifecycle change evidence
-- ============================================================================

-- One row records the complete accepted command and whether it changed the
-- current state. The stable actor row remains present in both lifecycle states.
CREATE TABLE identity.actor_lifecycle_changes (
    change_id TEXT NOT NULL,
    administrator_actor_id TEXT NOT NULL,
    target_actor_id TEXT NOT NULL,
    change_kind TEXT NOT NULL,
    recorded_at_ms BIGINT NOT NULL,
    state_changed BOOLEAN NOT NULL,

    CONSTRAINT pk_actor_lifecycle_changes
        PRIMARY KEY (change_id),

    CONSTRAINT fk_actor_lifecycle_changes_administrator
        FOREIGN KEY (administrator_actor_id)
        REFERENCES identity.actors (actor_id),

    CONSTRAINT fk_actor_lifecycle_changes_target
        FOREIGN KEY (target_actor_id)
        REFERENCES identity.actors (actor_id),

    CONSTRAINT ck_actor_lifecycle_changes_id
        CHECK (metadata.is_valid_identifier(change_id)),

    CONSTRAINT ck_actor_lifecycle_changes_kind
        CHECK (change_kind IN ('activate', 'deactivate'))
);

-- PostgreSQL does not automatically index foreign-key referencing columns.
-- These indexes support later actor-oriented audit and lifecycle queries.
CREATE INDEX ix_actor_lifecycle_changes_administrator
ON identity.actor_lifecycle_changes (administrator_actor_id);

CREATE INDEX ix_actor_lifecycle_changes_target
ON identity.actor_lifecycle_changes (target_actor_id);


-- ============================================================================
-- Immutability enforcement
-- ============================================================================

CREATE FUNCTION identity.reject_actor_lifecycle_change_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'actor lifecycle change evidence is immutable'
        USING ERRCODE = '23514',
              CONSTRAINT = 'ck_actor_lifecycle_changes_immutable';
END;
$$;

CREATE TRIGGER trg_actor_lifecycle_changes_immutable
BEFORE UPDATE OR DELETE ON identity.actor_lifecycle_changes
FOR EACH ROW
EXECUTE FUNCTION identity.reject_actor_lifecycle_change_mutation();


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON TABLE identity.actor_lifecycle_changes IS
    'Immutable audit of idempotent actor activation and deactivation commands and their resulting state effects.';

COMMENT ON COLUMN identity.actor_lifecycle_changes.change_id IS
    'Platform-scoped idempotency identity; reuse with different content is a collision.';

COMMENT ON COLUMN identity.actor_lifecycle_changes.administrator_actor_id IS
    'Authenticated actor whose identity.manage authorization was consumed.';

COMMENT ON COLUMN identity.actor_lifecycle_changes.target_actor_id IS
    'Stable internal actor whose activation state was changed or confirmed.';

COMMENT ON COLUMN identity.actor_lifecycle_changes.change_kind IS
    'Requested operation: activate permits new authentication and authorization; deactivate rejects them.';

COMMENT ON COLUMN identity.actor_lifecycle_changes.recorded_at_ms IS
    'Trusted host acceptance time as signed Unix epoch milliseconds.';

COMMENT ON COLUMN identity.actor_lifecycle_changes.state_changed IS
    'True when current actor state changed; false when the requested state already held.';

COMMENT ON FUNCTION identity.reject_actor_lifecycle_change_mutation() IS
    'Rejects update and delete attempts against immutable actor lifecycle evidence.';
