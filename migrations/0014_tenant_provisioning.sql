-- ============================================================================
-- FleetIQ atomic tenant provisioning
-- ============================================================================
--
-- Records each platform-authorized tenant provisioning command as immutable
-- audit evidence. The application writes this record, the tenant, and the
-- initial administrator's exact grants in one transaction.
--
-- The provisioning identity supports retry after an ambiguous response. The
-- unique tenant identity is a second arbiter: two different commands cannot
-- claim the same tenant. Exact retries must match every immutable command field.
-- ============================================================================


-- ============================================================================
-- Immutable tenant provisioning commands
-- ============================================================================

CREATE TABLE identity.tenant_provisionings (
    provisioning_id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    provisioner_actor_id TEXT NOT NULL,
    initial_administrator_actor_id TEXT NOT NULL,
    recorded_at_ms BIGINT NOT NULL,

    CONSTRAINT pk_tenant_provisionings
        PRIMARY KEY (provisioning_id),

    -- One tenant has one authoritative provisioning fact. This unique key also
    -- serializes concurrent commands that use different provisioning IDs for
    -- the same tenant.
    CONSTRAINT uq_tenant_provisionings_tenant
        UNIQUE (tenant_id),

    -- The repository inserts immutable audit evidence before the tenant row so
    -- both unique arbiters can resolve retries and collisions. Deferring this
    -- reference until commit permits that order without weakening atomicity.
    CONSTRAINT fk_tenant_provisionings_tenant
        FOREIGN KEY (tenant_id)
        REFERENCES metadata.tenants (tenant_id)
        DEFERRABLE INITIALLY DEFERRED,

    CONSTRAINT fk_tenant_provisionings_provisioner
        FOREIGN KEY (provisioner_actor_id)
        REFERENCES identity.actors (actor_id),

    CONSTRAINT fk_tenant_provisionings_initial_administrator
        FOREIGN KEY (initial_administrator_actor_id)
        REFERENCES identity.actors (actor_id),

    CONSTRAINT ck_tenant_provisionings_id
        CHECK (metadata.is_valid_identifier(provisioning_id))
);


-- PostgreSQL does not create indexes for referencing foreign-key columns.
-- These indexes keep future actor lifecycle checks from scanning the complete
-- provisioning audit history.
CREATE INDEX ix_tenant_provisionings_provisioner
ON identity.tenant_provisionings (provisioner_actor_id);

CREATE INDEX ix_tenant_provisionings_initial_administrator
ON identity.tenant_provisionings (initial_administrator_actor_id);


-- Provisioning records are immutable audit evidence. Corrections require a
-- separate lifecycle command rather than rewriting how a tenant was created.
CREATE FUNCTION identity.prevent_tenant_provisioning_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Tenant provisioning audit rows are immutable; % is not permitted on %.%',
        TG_OP,
        TG_TABLE_SCHEMA,
        TG_TABLE_NAME;
END;
$$;

CREATE TRIGGER tenant_provisionings_prevent_mutation
BEFORE UPDATE OR DELETE ON identity.tenant_provisionings
FOR EACH ROW
EXECUTE FUNCTION identity.prevent_tenant_provisioning_mutation();


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON TABLE identity.tenant_provisionings IS
    'Immutable audit of idempotent platform-authorized tenant provisioning commands.';

COMMENT ON COLUMN identity.tenant_provisionings.provisioning_id IS
    'Platform-scoped idempotency identity; reuse with different content is an application-level collision.';

COMMENT ON COLUMN identity.tenant_provisionings.tenant_id IS
    'New tenant identity and a second unique arbiter for provisioning commands.';

COMMENT ON COLUMN identity.tenant_provisionings.provisioner_actor_id IS
    'Authenticated actor whose ProvisionTenant authorization permitted the command.';

COMMENT ON COLUMN identity.tenant_provisionings.initial_administrator_actor_id IS
    'Existing internal actor receiving the initial read, metadata-management, and access-management grants.';

COMMENT ON COLUMN identity.tenant_provisionings.recorded_at_ms IS
    'Trusted host time in signed Unix milliseconds when the provisioning command was accepted.';

COMMENT ON FUNCTION identity.prevent_tenant_provisioning_mutation() IS
    'Rejects UPDATE and DELETE operations against immutable tenant provisioning audit evidence.';
