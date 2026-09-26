-- ============================================================================
-- FleetIQ platform action grants
-- ============================================================================
--
-- Tenant-scoped authorization cannot protect operations that create a tenant:
-- no tenant boundary exists before provisioning succeeds. This migration adds
-- a separate, deliberately small platform authorization vocabulary so those
-- operations cannot be inferred from tenant membership or tenant grants.
--
-- A platform grant represents current authorization state. Grant lifecycle,
-- administrative audit, initial operator bootstrap, and tenant provisioning
-- are separate capabilities. Until those are implemented, operators may seed
-- development data directly; product routes must not mutate this table.
-- ============================================================================


-- ============================================================================
-- Exact platform action grants
-- ============================================================================

-- Stores one explicit authorization fact for an exact internal actor and
-- platform action. There are no wildcards, hierarchy, implied grants, or
-- relationship to tenant_action_grants.
CREATE TABLE identity.platform_action_grants (
    actor_id TEXT NOT NULL,

    -- Values mirror the closed PlatformAction enum. Every new platform action
    -- requires a forward migration and exhaustive policy-adapter update.
    action TEXT NOT NULL,

    CONSTRAINT pk_platform_action_grants
        PRIMARY KEY (actor_id, action),

    CONSTRAINT fk_platform_action_grants_actor
        FOREIGN KEY (actor_id)
        REFERENCES identity.actors (actor_id),

    CONSTRAINT ck_platform_action_grants_action
        CHECK (
            action IN (
                'tenant.provision'
            )
        )
);


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON TABLE identity.platform_action_grants IS
    'Exact current grants for an internal actor to perform one platform-wide action.';

COMMENT ON COLUMN identity.platform_action_grants.actor_id IS
    'Internal actor receiving the exact platform grant; actor existence alone grants no authority.';

COMMENT ON COLUMN identity.platform_action_grants.action IS
    'Closed platform action identifier evaluated without tenant scope, wildcard, or hierarchy.';
