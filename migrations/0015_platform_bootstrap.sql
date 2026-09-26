-- ============================================================================
-- FleetIQ one-time platform bootstrap evidence
-- ============================================================================
--
-- A new installation has no internal actor that can hold a platform grant, so
-- ordinary authenticated administration cannot establish the first authority.
-- This migration records the single trusted bootstrap operation that creates:
--
--   1. the first internal platform operator;
--   2. the operator's exact external-identity mapping; and
--   3. the operator's initial tenant.provision grant.
--
-- The application writes this evidence and all dependent identity state in one
-- transaction. The singleton key is the durable concurrency arbiter: exactly
-- one bootstrap request can initialize an installation, while an exact retry
-- can resolve to the immutable record. Bootstrap is platform-wide and therefore
-- has no tenant key.
-- ============================================================================


-- ============================================================================
-- Immutable bootstrap evidence
-- ============================================================================

-- PostgreSQL requires the complete referenced column set to be declared unique
-- before bootstrap evidence can prove that its external identity belongs to
-- the same actor. The existing two-column primary key remains the identity
-- mapping's lookup key; this redundant declaration enables the stronger
-- three-column foreign key below.
ALTER TABLE identity.external_actor_identities
ADD CONSTRAINT uq_external_actor_identities_complete
UNIQUE (authority_id, subject, actor_id);

-- Stores the only initial-authority bootstrap accepted by this installation.
-- The Boolean primary key deliberately admits only TRUE, making the one-row
-- invariant visible to both PostgreSQL and readers of the schema.
CREATE TABLE identity.platform_bootstrap (
    singleton BOOLEAN NOT NULL DEFAULT TRUE,

    actor_id TEXT NOT NULL,
    authority_id TEXT NOT NULL,

    -- The provider subject remains exact, opaque, and case-sensitive. Its
    -- validation matches identity.external_actor_identities.
    subject TEXT NOT NULL,

    -- Trusted host time uses the existing signed Unix-millisecond contract.
    recorded_at_ms BIGINT NOT NULL,

    CONSTRAINT pk_platform_bootstrap
        PRIMARY KEY (singleton),

    CONSTRAINT ck_platform_bootstrap_singleton
        CHECK (singleton),

    CONSTRAINT ck_platform_bootstrap_actor
        CHECK (metadata.is_valid_identifier(actor_id)),

    CONSTRAINT ck_platform_bootstrap_authority
        CHECK (metadata.is_valid_identifier(authority_id)),

    CONSTRAINT ck_platform_bootstrap_subject
        CHECK (
            char_length(subject) BETWEEN 1 AND 512
            AND btrim(subject) <> ''
            AND subject !~ '[[:cntrl:]]'
        ),

    -- Both references are deferred so the immutable bootstrap claim can be the
    -- transaction's first write and serialize contenders before creating the
    -- actor and external-identity mapping.
    CONSTRAINT fk_platform_bootstrap_actor
        FOREIGN KEY (actor_id)
        REFERENCES identity.actors (actor_id)
        DEFERRABLE INITIALLY DEFERRED,

    CONSTRAINT fk_platform_bootstrap_external_identity
        FOREIGN KEY (authority_id, subject, actor_id)
        REFERENCES identity.external_actor_identities (authority_id, subject, actor_id)
        DEFERRABLE INITIALLY DEFERRED
);


-- ============================================================================
-- Immutability enforcement
-- ============================================================================

-- Bootstrap evidence explains the root of platform authority and must not be
-- rewritten or removed after dependent identity state has been established.
CREATE FUNCTION identity.reject_platform_bootstrap_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'platform bootstrap evidence is immutable'
        USING ERRCODE = '23514',
              CONSTRAINT = 'ck_platform_bootstrap_immutable';
END;
$$;

CREATE TRIGGER trg_platform_bootstrap_immutable
BEFORE UPDATE OR DELETE ON identity.platform_bootstrap
FOR EACH ROW
EXECUTE FUNCTION identity.reject_platform_bootstrap_mutation();


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON TABLE identity.platform_bootstrap IS
    'The single immutable operation that established the installation''s first platform operator, external identity, and tenant-provisioning authority.';

COMMENT ON COLUMN identity.platform_bootstrap.singleton IS
    'Constant TRUE key that permits exactly one bootstrap record per installation.';

COMMENT ON COLUMN identity.platform_bootstrap.actor_id IS
    'Stable internal identity created for the initial platform operator.';

COMMENT ON COLUMN identity.platform_bootstrap.authority_id IS
    'Trusted authentication-authority configuration bound during bootstrap.';

COMMENT ON COLUMN identity.platform_bootstrap.subject IS
    'Exact opaque external subject bound to the initial platform operator.';

COMMENT ON COLUMN identity.platform_bootstrap.recorded_at_ms IS
    'Trusted host acceptance time as signed Unix epoch milliseconds.';

COMMENT ON FUNCTION identity.reject_platform_bootstrap_mutation() IS
    'Rejects update and delete attempts against immutable platform-bootstrap evidence.';
