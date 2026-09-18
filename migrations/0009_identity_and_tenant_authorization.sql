-- ============================================================================
-- FleetIQ internal actors and tenant action grants
-- ============================================================================
--
-- This migration introduces the durable source used by the application tenant
-- authorization boundary:
--
--   1. Internal actors
--
--      An actor is FleetIQ's stable identity for an authenticated human or
--      workload. Provider subjects, issuers, credentials, and sessions are not
--      stored here; a later identity adapter maps those external identities to
--      this internal key.
--
--   2. Exact tenant action grants
--
--      A grant permits one actor to perform one application-defined action in
--      one tenant. There is no implicit action hierarchy, wildcard, default
--      tenant, or grant derived merely from actor or tenant existence.
--
-- Grants describe current authorization state and may be inserted or deleted.
-- A later administrative slice will own those mutations and their audit facts.
-- The backend currently connects through one application database role, so
-- PostgreSQL row-level security is not enabled by this migration. Product
-- authorization remains enforced by the application policy boundary.
-- ============================================================================


-- ============================================================================
-- Identity namespace
-- ============================================================================

-- Internal identities and authorization state are isolated from semantic asset
-- metadata so their ownership, privileges, and lifecycle can evolve separately.
CREATE SCHEMA identity;


-- ============================================================================
-- Internal actors
-- ============================================================================

-- Records stable FleetIQ identities after provisioning or external-identity
-- mapping. A row proves only that the actor exists; it grants no tenant access.
CREATE TABLE identity.actors (
    actor_id TEXT PRIMARY KEY,

    CONSTRAINT ck_actors_id
        CHECK (metadata.is_valid_identifier(actor_id))
);


-- ============================================================================
-- Tenant action grants
-- ============================================================================

-- Stores one explicit authorization fact for an exact actor, tenant, and
-- application action tuple. The primary key both prevents duplicate grants and
-- supports the policy adapter's complete-tuple existence query.
CREATE TABLE identity.tenant_action_grants (
    tenant_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,

    -- Action values mirror the closed TenantAction enum. Adding an application
    -- action requires a forward migration and an exhaustive adapter update so
    -- the new capability cannot silently inherit an existing grant.
    action TEXT NOT NULL,

    CONSTRAINT pk_tenant_action_grants
        PRIMARY KEY (tenant_id, actor_id, action),

    CONSTRAINT fk_tenant_action_grants_tenant
        FOREIGN KEY (tenant_id)
        REFERENCES metadata.tenants (tenant_id),

    CONSTRAINT fk_tenant_action_grants_actor
        FOREIGN KEY (actor_id)
        REFERENCES identity.actors (actor_id),

    CONSTRAINT ck_tenant_action_grants_action
        CHECK (
            action IN (
                'metadata.read',
                'metadata.manage',
                'access.manage'
            )
        )
);


-- The primary key begins with tenant_id and supports policy evaluation and
-- tenant deletion checks. PostgreSQL does not automatically index the
-- referencing side of the actor foreign key, so this index prevents actor
-- lifecycle checks from scanning every tenant's grants.
CREATE INDEX ix_tenant_action_grants_actor
ON identity.tenant_action_grants (actor_id);


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON SCHEMA identity IS
    'Internal actors and current tenant authorization state for FleetIQ.';

COMMENT ON TABLE identity.actors IS
    'Stable internal identities for authenticated humans and workloads; existence grants no tenant access.';

COMMENT ON TABLE identity.tenant_action_grants IS
    'Exact current grants for an actor to perform one application action in one tenant.';

COMMENT ON COLUMN identity.actors.actor_id IS
    'Stable FleetIQ identity independent of an external identity provider.';

COMMENT ON COLUMN identity.tenant_action_grants.tenant_id IS
    'Tenant in which the exact action is granted; selection of this value alone proves no access.';

COMMENT ON COLUMN identity.tenant_action_grants.actor_id IS
    'Internal actor receiving the exact tenant-scoped grant.';

COMMENT ON COLUMN identity.tenant_action_grants.action IS
    'Closed application action identifier evaluated without wildcard or hierarchy semantics.';
