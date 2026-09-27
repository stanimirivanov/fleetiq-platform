-- ============================================================================
-- FleetIQ platform access administration
-- ============================================================================
--
-- Introduces a distinct authority for changing platform-wide grants and an
-- immutable audit record for each idempotent grant or revoke command. Platform
-- access remains independent from actor existence and tenant-scoped access.
--
-- The repository serializes these low-volume operations through the existing
-- immutable bootstrap singleton. Current grant state and its audit evidence
-- commit in one transaction, so neither can exist without the other.
-- ============================================================================


-- ============================================================================
-- Platform grant-administration authority
-- ============================================================================

-- Replace the closed vocabulary constraint explicitly. The new authority is
-- not implied by identity.manage, tenant.provision, or a tenant-scoped grant.
ALTER TABLE identity.platform_action_grants
DROP CONSTRAINT ck_platform_action_grants_action;

ALTER TABLE identity.platform_action_grants
ADD CONSTRAINT ck_platform_action_grants_action
CHECK (
    action IN (
        'identity.manage',
        'platform.access.manage',
        'tenant.provision'
    )
);

-- Only the immutable bootstrap operator receives the new authority during
-- migration. Existing delegated platform operators are not silently elevated.
INSERT INTO identity.platform_action_grants (actor_id, action)
SELECT actor_id, 'platform.access.manage'
FROM identity.platform_bootstrap
ON CONFLICT DO NOTHING;


-- ============================================================================
-- Immutable platform access audit
-- ============================================================================

-- Stores one platform-scoped command and the state effect produced by its
-- atomic mutation. A repeated change ID must retain every field exactly.
CREATE TABLE identity.platform_access_changes (
    change_id TEXT NOT NULL,
    administrator_actor_id TEXT NOT NULL,
    target_actor_id TEXT NOT NULL,
    action TEXT NOT NULL,
    change_kind TEXT NOT NULL,
    recorded_at_ms BIGINT NOT NULL,
    state_changed BOOLEAN NOT NULL,

    CONSTRAINT pk_platform_access_changes
        PRIMARY KEY (change_id),

    CONSTRAINT fk_platform_access_changes_administrator
        FOREIGN KEY (administrator_actor_id)
        REFERENCES identity.actors (actor_id),

    CONSTRAINT fk_platform_access_changes_target
        FOREIGN KEY (target_actor_id)
        REFERENCES identity.actors (actor_id),

    CONSTRAINT ck_platform_access_changes_id
        CHECK (metadata.is_valid_identifier(change_id)),

    -- Audit vocabulary duplicates the current grant vocabulary deliberately.
    -- Historical rows retain the exact action accepted at command time even if
    -- a later migration changes which actions may receive new grants.
    CONSTRAINT ck_platform_access_changes_action
        CHECK (
            action IN (
                'identity.manage',
                'platform.access.manage',
                'tenant.provision'
            )
        ),

    CONSTRAINT ck_platform_access_changes_kind
        CHECK (change_kind IN ('grant', 'revoke'))
);


-- PostgreSQL does not automatically index referencing foreign-key columns.
-- These indexes support actor lifecycle checks and later audit queries by the
-- authenticated administrator or affected actor.
CREATE INDEX ix_platform_access_changes_administrator
ON identity.platform_access_changes (administrator_actor_id);

CREATE INDEX ix_platform_access_changes_target
ON identity.platform_access_changes (target_actor_id);


-- ============================================================================
-- Immutability enforcement
-- ============================================================================

-- A correction or compensating action receives a new change identity rather
-- than rewriting the original accepted command or its observed effect.
CREATE FUNCTION identity.reject_platform_access_change_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'platform access audit evidence is immutable'
        USING ERRCODE = '23514',
              CONSTRAINT = 'ck_platform_access_changes_immutable';
END;
$$;

CREATE TRIGGER trg_platform_access_changes_immutable
BEFORE UPDATE OR DELETE ON identity.platform_access_changes
FOR EACH ROW
EXECUTE FUNCTION identity.reject_platform_access_change_mutation();


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON TABLE identity.platform_access_changes IS
    'Immutable audit of idempotent platform grant and revoke commands and their resulting state effects.';

COMMENT ON COLUMN identity.platform_access_changes.change_id IS
    'Platform-scoped idempotency identity; reuse with different content is a collision.';

COMMENT ON COLUMN identity.platform_access_changes.administrator_actor_id IS
    'Authenticated actor whose platform.access.manage authorization was consumed.';

COMMENT ON COLUMN identity.platform_access_changes.target_actor_id IS
    'Actor whose exact platform action grant was changed or confirmed.';

COMMENT ON COLUMN identity.platform_access_changes.action IS
    'Exact closed-vocabulary platform action requested by the command.';

COMMENT ON COLUMN identity.platform_access_changes.change_kind IS
    'Requested operation: grant ensures presence and revoke ensures absence.';

COMMENT ON COLUMN identity.platform_access_changes.recorded_at_ms IS
    'Trusted host acceptance time as signed Unix epoch milliseconds.';

COMMENT ON COLUMN identity.platform_access_changes.state_changed IS
    'True when current grant state changed; false when the requested state already held.';

COMMENT ON FUNCTION identity.reject_platform_access_change_mutation() IS
    'Rejects update and delete attempts against immutable platform access audit evidence.';
