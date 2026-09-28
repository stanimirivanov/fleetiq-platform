-- ============================================================================
-- FleetIQ tenant lifecycle administration
-- ============================================================================
--
-- Adds reversible tenant operational state, a dedicated platform authority,
-- and immutable evidence for authorized activation and suspension commands.
-- Suspension preserves tenant-owned data and grants while making fresh tenant
-- authorization decisions fail closed.
--
-- Operational note: replacing the closed action constraints and adding the
-- NOT NULL column require brief table locks. The constant TRUE default does
-- not rewrite existing tenant rows on PostgreSQL 18.
-- ============================================================================


-- ============================================================================
-- Platform tenant-lifecycle authority
-- ============================================================================

ALTER TABLE identity.platform_action_grants
DROP CONSTRAINT ck_platform_action_grants_action;

ALTER TABLE identity.platform_action_grants
ADD CONSTRAINT ck_platform_action_grants_action
CHECK (
    action IN (
        'identity.manage',
        'platform.access.manage',
        'tenant.lifecycle.manage',
        'tenant.provision'
    )
);

ALTER TABLE identity.platform_access_changes
DROP CONSTRAINT ck_platform_access_changes_action;

ALTER TABLE identity.platform_access_changes
ADD CONSTRAINT ck_platform_access_changes_action
CHECK (
    action IN (
        'identity.manage',
        'platform.access.manage',
        'tenant.lifecycle.manage',
        'tenant.provision'
    )
);

-- Existing installations explicitly delegate the new exact action only to
-- the immutable bootstrap operator. Later delegation uses the normal audited
-- platform-access administration command.
INSERT INTO identity.platform_action_grants (actor_id, action)
SELECT actor_id, 'tenant.lifecycle.manage'
FROM identity.platform_bootstrap
ON CONFLICT DO NOTHING;


-- ============================================================================
-- Current tenant lifecycle state
-- ============================================================================

ALTER TABLE metadata.tenants
ADD COLUMN is_active BOOLEAN NOT NULL DEFAULT TRUE;

COMMENT ON COLUMN metadata.tenants.is_active IS
    'Whether current tenant grants may authorize fresh operations in this ownership boundary.';


-- ============================================================================
-- Immutable tenant lifecycle change evidence
-- ============================================================================

CREATE TABLE identity.tenant_lifecycle_changes (
    change_id TEXT NOT NULL,
    administrator_actor_id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    change_kind TEXT NOT NULL,
    recorded_at_ms BIGINT NOT NULL,
    state_changed BOOLEAN NOT NULL,

    CONSTRAINT pk_tenant_lifecycle_changes
        PRIMARY KEY (change_id),

    CONSTRAINT fk_tenant_lifecycle_changes_administrator
        FOREIGN KEY (administrator_actor_id)
        REFERENCES identity.actors (actor_id),

    CONSTRAINT fk_tenant_lifecycle_changes_tenant
        FOREIGN KEY (tenant_id)
        REFERENCES metadata.tenants (tenant_id),

    CONSTRAINT ck_tenant_lifecycle_changes_id
        CHECK (metadata.is_valid_identifier(change_id)),

    CONSTRAINT ck_tenant_lifecycle_changes_kind
        CHECK (change_kind IN ('activate', 'suspend'))
);

-- PostgreSQL does not automatically index foreign-key referencing columns.
CREATE INDEX ix_tenant_lifecycle_changes_administrator
ON identity.tenant_lifecycle_changes (administrator_actor_id);

CREATE INDEX ix_tenant_lifecycle_changes_tenant
ON identity.tenant_lifecycle_changes (tenant_id);


-- ============================================================================
-- Immutability enforcement
-- ============================================================================

CREATE FUNCTION identity.reject_tenant_lifecycle_change_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'tenant lifecycle change evidence is immutable'
        USING ERRCODE = '23514',
              CONSTRAINT = 'ck_tenant_lifecycle_changes_immutable';
END;
$$;

CREATE TRIGGER trg_tenant_lifecycle_changes_immutable
BEFORE UPDATE OR DELETE ON identity.tenant_lifecycle_changes
FOR EACH ROW
EXECUTE FUNCTION identity.reject_tenant_lifecycle_change_mutation();


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON TABLE identity.tenant_lifecycle_changes IS
    'Immutable audit of idempotent tenant activation and suspension commands and their resulting state effects.';

COMMENT ON COLUMN identity.tenant_lifecycle_changes.change_id IS
    'Platform-scoped idempotency identity; reuse with different content is a collision.';

COMMENT ON COLUMN identity.tenant_lifecycle_changes.administrator_actor_id IS
    'Authenticated actor whose tenant.lifecycle.manage authorization was consumed.';

COMMENT ON COLUMN identity.tenant_lifecycle_changes.tenant_id IS
    'Stable ownership boundary whose operational state was changed or confirmed.';

COMMENT ON COLUMN identity.tenant_lifecycle_changes.change_kind IS
    'Requested operation: activate permits fresh tenant authorization and suspend denies it.';

COMMENT ON COLUMN identity.tenant_lifecycle_changes.recorded_at_ms IS
    'Trusted host acceptance time as signed Unix epoch milliseconds.';

COMMENT ON COLUMN identity.tenant_lifecycle_changes.state_changed IS
    'True when current tenant state changed; false when the requested state already held.';

COMMENT ON FUNCTION identity.reject_tenant_lifecycle_change_mutation() IS
    'Rejects update and delete attempts against immutable tenant lifecycle evidence.';
