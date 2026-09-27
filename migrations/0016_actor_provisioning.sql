-- ============================================================================
-- FleetIQ authorized actor provisioning
-- ============================================================================
--
-- This migration extends the closed platform-action vocabulary and records
-- idempotent creation of internal actors with their first external identity.
-- Actor existence and identity mapping grant no tenant or platform access.
--
-- The application writes immutable provisioning evidence, the actor, and the
-- exact external-identity mapping in one transaction. Three unique arbiters
-- serialize reuse of the command ID, actor ID, or external identity before any
-- dependent state commits.
-- ============================================================================


-- ============================================================================
-- Platform identity-administration authority
-- ============================================================================

-- Replace the closed vocabulary constraint explicitly. Adding identity.manage
-- does not make it implied by tenant.provision or by any tenant-scoped grant.
ALTER TABLE identity.platform_action_grants
DROP CONSTRAINT ck_platform_action_grants_action;

ALTER TABLE identity.platform_action_grants
ADD CONSTRAINT ck_platform_action_grants_action
CHECK (
    action IN (
        'identity.manage',
        'tenant.provision'
    )
);

-- The bootstrap operator is the installation's existing root authority. This
-- forward migration explicitly grants the newly introduced action to that
-- actor. It does not grant the action to any other current platform operator.
INSERT INTO identity.platform_action_grants (actor_id, action)
SELECT actor_id, 'identity.manage'
FROM identity.platform_bootstrap
ON CONFLICT DO NOTHING;


-- ============================================================================
-- Immutable actor-provisioning evidence
-- ============================================================================

-- Records the complete accepted meaning of one actor provisioning operation.
-- The unique actor and external-identity constraints are additional command
-- arbiters: neither durable identity can be claimed by competing commands.
CREATE TABLE identity.actor_provisionings (
    provisioning_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    authority_id TEXT NOT NULL,

    -- Provider subjects remain exact, opaque, and case-sensitive. Validation
    -- matches identity.external_actor_identities.
    subject TEXT NOT NULL,

    provisioner_actor_id TEXT NOT NULL,

    -- Trusted host time follows the existing signed Unix-millisecond contract.
    recorded_at_ms BIGINT NOT NULL,

    CONSTRAINT pk_actor_provisionings
        PRIMARY KEY (provisioning_id),

    CONSTRAINT uq_actor_provisionings_actor
        UNIQUE (actor_id),

    CONSTRAINT uq_actor_provisionings_external_identity
        UNIQUE (authority_id, subject),

    CONSTRAINT ck_actor_provisionings_id
        CHECK (metadata.is_valid_identifier(provisioning_id)),

    CONSTRAINT ck_actor_provisionings_actor
        CHECK (metadata.is_valid_identifier(actor_id)),

    CONSTRAINT ck_actor_provisionings_authority
        CHECK (metadata.is_valid_identifier(authority_id)),

    CONSTRAINT ck_actor_provisionings_subject
        CHECK (
            char_length(subject) BETWEEN 1 AND 512
            AND btrim(subject) <> ''
            AND subject !~ '[[:cntrl:]]'
        ),

    -- These references are deferred so the audit row can claim all command
    -- identities before the actor and mapping are inserted in the transaction.
    CONSTRAINT fk_actor_provisionings_actor
        FOREIGN KEY (actor_id)
        REFERENCES identity.actors (actor_id)
        DEFERRABLE INITIALLY DEFERRED,

    CONSTRAINT fk_actor_provisionings_external_identity
        FOREIGN KEY (authority_id, subject, actor_id)
        REFERENCES identity.external_actor_identities (authority_id, subject, actor_id)
        DEFERRABLE INITIALLY DEFERRED,

    CONSTRAINT fk_actor_provisionings_provisioner
        FOREIGN KEY (provisioner_actor_id)
        REFERENCES identity.actors (actor_id)
);


-- PostgreSQL does not automatically index the referencing side of the
-- provisioner foreign key. This index supports later actor-lifecycle checks and
-- audit queries by the authenticated administrator.
CREATE INDEX ix_actor_provisionings_provisioner
ON identity.actor_provisionings (provisioner_actor_id);


-- ============================================================================
-- Immutability enforcement
-- ============================================================================

CREATE FUNCTION identity.reject_actor_provisioning_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'actor provisioning evidence is immutable'
        USING ERRCODE = '23514',
              CONSTRAINT = 'ck_actor_provisionings_immutable';
END;
$$;

CREATE TRIGGER trg_actor_provisionings_immutable
BEFORE UPDATE OR DELETE ON identity.actor_provisionings
FOR EACH ROW
EXECUTE FUNCTION identity.reject_actor_provisioning_mutation();


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON TABLE identity.actor_provisionings IS
    'Immutable platform-authorized operations that created an internal actor and its first exact external identity.';

COMMENT ON COLUMN identity.actor_provisionings.provisioning_id IS
    'Platform-scoped idempotency identity assigned by the trusted host.';

COMMENT ON COLUMN identity.actor_provisionings.actor_id IS
    'Stable internal actor created by this provisioning operation.';

COMMENT ON COLUMN identity.actor_provisionings.authority_id IS
    'Trusted authentication-authority configuration for the initial external identity.';

COMMENT ON COLUMN identity.actor_provisionings.subject IS
    'Exact opaque case-sensitive external subject initially bound to the actor.';

COMMENT ON COLUMN identity.actor_provisionings.provisioner_actor_id IS
    'Authenticated platform actor whose identity.manage authorization was consumed.';

COMMENT ON COLUMN identity.actor_provisionings.recorded_at_ms IS
    'Trusted host acceptance time as signed Unix epoch milliseconds.';

COMMENT ON FUNCTION identity.reject_actor_provisioning_mutation() IS
    'Rejects update and delete attempts against immutable actor-provisioning evidence.';
