-- ============================================================================
-- FleetIQ external identity to internal actor mapping
-- ============================================================================
--
-- Maps a principal established by a trusted authentication adapter to the
-- stable internal actor used by application authorization. The composite
-- external key contains:
--
--   1. an authority ID naming trusted authentication configuration; and
--   2. the exact opaque subject asserted by that authority.
--
-- A caller-supplied authority or subject is not authentication evidence. Token,
-- certificate, or session validation remains outside this table. One external
-- identity maps to exactly one actor, while one actor may deliberately have
-- identities in multiple authorities. Mapping existence grants no tenant
-- action; tenant_action_grants remains the authorization source of truth.
-- ============================================================================


-- Stores the exact many-to-one mapping from external principals to internal
-- actors. The authority is a FleetIQ configuration identity rather than a raw
-- provider URL, keeping provider configuration and key rotation outside the
-- durable principal key.
CREATE TABLE identity.external_actor_identities (
    authority_id TEXT NOT NULL,

    -- Subjects are opaque and case-sensitive. They are preserved without
    -- normalization after bounded validation, because providers define their
    -- comparison semantics and changes could merge distinct principals.
    subject TEXT NOT NULL,

    actor_id TEXT NOT NULL,

    CONSTRAINT pk_external_actor_identities
        PRIMARY KEY (authority_id, subject),

    CONSTRAINT fk_external_actor_identities_actor
        FOREIGN KEY (actor_id)
        REFERENCES identity.actors (actor_id),

    CONSTRAINT ck_external_actor_identities_authority
        CHECK (metadata.is_valid_identifier(authority_id)),

    CONSTRAINT ck_external_actor_identities_subject
        CHECK (
            char_length(subject) BETWEEN 1 AND 512
            AND btrim(subject) <> ''
            AND subject !~ '[[:cntrl:]]'
        )
);


-- PostgreSQL does not automatically index the referencing side of a foreign
-- key. This index keeps actor lifecycle checks and actor-oriented mapping
-- administration from scanning every external identity.
CREATE INDEX ix_external_actor_identities_actor
ON identity.external_actor_identities (actor_id);


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON TABLE identity.external_actor_identities IS
    'Exact mappings from externally authenticated principals to stable internal FleetIQ actors; mapping existence grants no tenant access.';

COMMENT ON COLUMN identity.external_actor_identities.authority_id IS
    'Stable FleetIQ identity of the trusted authentication authority configuration that established the subject.';

COMMENT ON COLUMN identity.external_actor_identities.subject IS
    'Exact opaque case-sensitive principal subject asserted by the trusted authority.';

COMMENT ON COLUMN identity.external_actor_identities.actor_id IS
    'Stable internal actor used for subsequent tenant authorization decisions.';
