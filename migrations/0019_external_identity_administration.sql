-- ============================================================================
-- FleetIQ external-identity administration
-- ============================================================================
--
-- Adds authorized, idempotent attach and detach commands for exact external
-- identities. Current mappings and immutable audit evidence commit together.
-- The migration also changes provisioning evidence from a permanent foreign
-- key to a deferred insert-time assertion. Provisioning still cannot commit
-- without the exact initial mapping, while later identity replacement may
-- detach that mapping without rewriting historical evidence.
--
-- Operational note: dropping foreign keys and installing triggers acquires
-- table locks on identity.platform_bootstrap and identity.actor_provisionings.
-- Schedule this migration during a deployment window that can tolerate a brief
-- pause in bootstrap and identity-provisioning writes.
-- ============================================================================


-- ============================================================================
-- Preserve provisioning-time identity validation without permanent pinning
-- ============================================================================

ALTER TABLE identity.platform_bootstrap
DROP CONSTRAINT fk_platform_bootstrap_external_identity;

ALTER TABLE identity.actor_provisionings
DROP CONSTRAINT fk_actor_provisionings_external_identity;

-- This deferred assertion runs only for new provisioning evidence. It verifies
-- the exact authority, subject, and actor at transaction commit, after the
-- provisioning repository has created the mapping. Later mapping lifecycle
-- does not invalidate the immutable fact describing what was provisioned.
CREATE FUNCTION identity.require_provisioned_external_identity()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    constraint_name TEXT;
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM identity.external_actor_identities
        WHERE authority_id = NEW.authority_id
          AND subject = NEW.subject
          AND actor_id = NEW.actor_id
    ) THEN
        -- Preserve the original foreign-key constraint names as the durable
        -- database-error contract even though enforcement now occurs only when
        -- the immutable provisioning fact is inserted.
        constraint_name := CASE TG_TABLE_NAME
            WHEN 'platform_bootstrap' THEN 'fk_platform_bootstrap_external_identity'
            WHEN 'actor_provisionings' THEN 'fk_actor_provisionings_external_identity'
            ELSE 'fk_provisioning_external_identity_at_insert'
        END;

        RAISE EXCEPTION 'provisioning evidence requires its exact external identity mapping'
            USING ERRCODE = '23503',
                  CONSTRAINT = constraint_name;
    END IF;

    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER trg_platform_bootstrap_external_identity_at_insert
AFTER INSERT ON identity.platform_bootstrap
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION identity.require_provisioned_external_identity();

CREATE CONSTRAINT TRIGGER trg_actor_provisionings_external_identity_at_insert
AFTER INSERT ON identity.actor_provisionings
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION identity.require_provisioned_external_identity();


-- ============================================================================
-- Immutable external-identity change audit
-- ============================================================================

-- One row records the complete accepted command and whether it changed the
-- current mapping. Subjects remain exact, opaque, and case-sensitive. They are
-- security-sensitive identifiers and must not be copied into routine errors or
-- logs merely because they are retained here as necessary audit evidence.
CREATE TABLE identity.external_identity_changes (
    change_id TEXT NOT NULL,
    administrator_actor_id TEXT NOT NULL,
    target_actor_id TEXT NOT NULL,
    authority_id TEXT NOT NULL,
    subject TEXT NOT NULL,
    change_kind TEXT NOT NULL,
    recorded_at_ms BIGINT NOT NULL,
    state_changed BOOLEAN NOT NULL,

    CONSTRAINT pk_external_identity_changes
        PRIMARY KEY (change_id),

    CONSTRAINT fk_external_identity_changes_administrator
        FOREIGN KEY (administrator_actor_id)
        REFERENCES identity.actors (actor_id),

    CONSTRAINT fk_external_identity_changes_target
        FOREIGN KEY (target_actor_id)
        REFERENCES identity.actors (actor_id),

    CONSTRAINT ck_external_identity_changes_id
        CHECK (metadata.is_valid_identifier(change_id)),

    CONSTRAINT ck_external_identity_changes_authority
        CHECK (metadata.is_valid_identifier(authority_id)),

    CONSTRAINT ck_external_identity_changes_subject
        CHECK (
            char_length(subject) BETWEEN 1 AND 512
            AND btrim(subject) <> ''
            AND subject !~ '[[:cntrl:]]'
        ),

    CONSTRAINT ck_external_identity_changes_kind
        CHECK (change_kind IN ('attach', 'detach'))
);

-- PostgreSQL does not automatically index the referencing side of foreign
-- keys. These indexes support later actor lifecycle and audit query work.
CREATE INDEX ix_external_identity_changes_administrator
ON identity.external_identity_changes (administrator_actor_id);

CREATE INDEX ix_external_identity_changes_target
ON identity.external_identity_changes (target_actor_id);


-- ============================================================================
-- Immutability enforcement
-- ============================================================================

-- Corrections use a new command identity. Accepted command meaning and its
-- observed effect are never updated or deleted in place.
CREATE FUNCTION identity.reject_external_identity_change_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'external identity change evidence is immutable'
        USING ERRCODE = '23514',
              CONSTRAINT = 'ck_external_identity_changes_immutable';
END;
$$;

CREATE TRIGGER trg_external_identity_changes_immutable
BEFORE UPDATE OR DELETE ON identity.external_identity_changes
FOR EACH ROW
EXECUTE FUNCTION identity.reject_external_identity_change_mutation();


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON FUNCTION identity.require_provisioned_external_identity() IS
    'At insertion commit, requires bootstrap or actor-provisioning evidence to reference its exact external identity mapping without permanently pinning that mapping.';

COMMENT ON TABLE identity.external_identity_changes IS
    'Immutable audit of idempotent external-identity attach and detach commands and their resulting mapping effects.';

COMMENT ON COLUMN identity.external_identity_changes.change_id IS
    'Platform-scoped idempotency identity; reuse with different content is a collision.';

COMMENT ON COLUMN identity.external_identity_changes.administrator_actor_id IS
    'Authenticated actor whose identity.manage authorization was consumed.';

COMMENT ON COLUMN identity.external_identity_changes.target_actor_id IS
    'Stable internal actor whose external identity mapping was changed or confirmed.';

COMMENT ON COLUMN identity.external_identity_changes.authority_id IS
    'Trusted authentication-authority configuration for the exact external identity.';

COMMENT ON COLUMN identity.external_identity_changes.subject IS
    'Exact opaque case-sensitive subject of the external identity.';

COMMENT ON COLUMN identity.external_identity_changes.change_kind IS
    'Requested operation: attach ensures the mapping exists and detach ensures it does not exist for the target actor.';

COMMENT ON COLUMN identity.external_identity_changes.recorded_at_ms IS
    'Trusted host acceptance time as signed Unix epoch milliseconds.';

COMMENT ON COLUMN identity.external_identity_changes.state_changed IS
    'True when current mapping state changed; false when the requested state already held.';

COMMENT ON FUNCTION identity.reject_external_identity_change_mutation() IS
    'Rejects update and delete attempts against immutable external-identity change evidence.';
