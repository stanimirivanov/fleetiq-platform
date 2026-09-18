-- ============================================================================
-- FleetIQ tenant access change audit
-- ============================================================================
--
-- Records every supported grant or revoke command with the authenticated
-- administrator, target actor, tenant, action, trusted host time, and resulting
-- state effect. The command identity makes retries idempotent and detects reuse
-- for different content. Current grant state and its audit row are written in
-- one repository transaction.
--
-- Audit rows are immutable. Direct database access remains an operational
-- privilege and can bypass application orchestration; database permissions and
-- operational controls must restrict such access.
-- ============================================================================


-- Stores one immutable access administration command and its durable effect.
CREATE TABLE identity.tenant_access_changes (
    tenant_id TEXT NOT NULL,
    change_id TEXT NOT NULL,
    administrator_actor_id TEXT NOT NULL,
    target_actor_id TEXT NOT NULL,
    action TEXT NOT NULL,
    change_kind TEXT NOT NULL,
    recorded_at_ms BIGINT NOT NULL,
    state_changed BOOLEAN NOT NULL,

    CONSTRAINT pk_tenant_access_changes
        PRIMARY KEY (tenant_id, change_id),

    CONSTRAINT fk_tenant_access_changes_tenant
        FOREIGN KEY (tenant_id)
        REFERENCES metadata.tenants (tenant_id),

    CONSTRAINT fk_tenant_access_changes_administrator
        FOREIGN KEY (administrator_actor_id)
        REFERENCES identity.actors (actor_id),

    CONSTRAINT fk_tenant_access_changes_target
        FOREIGN KEY (target_actor_id)
        REFERENCES identity.actors (actor_id),

    CONSTRAINT ck_tenant_access_changes_id
        CHECK (metadata.is_valid_identifier(change_id)),

    CONSTRAINT ck_tenant_access_changes_action
        CHECK (action IN ('metadata.read', 'metadata.manage', 'access.manage')),

    CONSTRAINT ck_tenant_access_changes_kind
        CHECK (change_kind IN ('grant', 'revoke'))
);


-- The primary key supports tenant-oriented history and tenant foreign-key
-- checks. These indexes support the two actor foreign keys and later audit
-- queries by administrator or affected actor without scanning all tenants.
CREATE INDEX ix_tenant_access_changes_administrator
ON identity.tenant_access_changes (administrator_actor_id);

CREATE INDEX ix_tenant_access_changes_target
ON identity.tenant_access_changes (target_actor_id);


-- Audit evidence is append-only. A correction or compensating action receives
-- a new change identity rather than rewriting the original event.
CREATE FUNCTION identity.prevent_tenant_access_change_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Tenant access audit rows are immutable; % is not permitted on %.%',
        TG_OP,
        TG_TABLE_SCHEMA,
        TG_TABLE_NAME;
END;
$$;

CREATE TRIGGER tenant_access_changes_prevent_mutation
BEFORE UPDATE OR DELETE ON identity.tenant_access_changes
FOR EACH ROW
EXECUTE FUNCTION identity.prevent_tenant_access_change_mutation();


COMMENT ON TABLE identity.tenant_access_changes IS
    'Immutable audit of idempotent tenant grant and revoke commands and their resulting state effects.';

COMMENT ON COLUMN identity.tenant_access_changes.change_id IS
    'Tenant-scoped idempotency identity; reuse with different content is an application-level collision.';

COMMENT ON COLUMN identity.tenant_access_changes.administrator_actor_id IS
    'Authenticated actor whose ManageAccess authorization permitted the command.';

COMMENT ON COLUMN identity.tenant_access_changes.target_actor_id IS
    'Actor whose exact tenant action grant was changed or confirmed.';

COMMENT ON COLUMN identity.tenant_access_changes.recorded_at_ms IS
    'Trusted host time in signed Unix milliseconds when the command was accepted.';

COMMENT ON COLUMN identity.tenant_access_changes.state_changed IS
    'True when current grant state changed; false when the requested state already held.';

COMMENT ON FUNCTION identity.prevent_tenant_access_change_mutation() IS
    'Rejects UPDATE and DELETE operations against immutable tenant access audit evidence.';
