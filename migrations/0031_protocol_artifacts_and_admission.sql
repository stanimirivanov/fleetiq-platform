-- Protocol artifact storage and explicit admission, separate from activation.
-- Initial bounded artifacts use PostgreSQL so a committed artifact is complete
-- and backed up with its catalogue. A release may remain metadata-only after an
-- interrupted upload; retries complete storage and admission requires bytes.
-- Existing catalogue keys and all previous migrations remain unchanged.
-- The two small authorization tables are briefly locked while their closed
-- action checks expand. No existing grant is upgraded or automatically added.

ALTER TABLE identity.tenant_action_grants DROP CONSTRAINT ck_tenant_action_grants_action;
ALTER TABLE identity.tenant_action_grants ADD CONSTRAINT ck_tenant_action_grants_action
    CHECK (action IN ('metadata.read', 'metadata.manage', 'access.manage', 'protocol.manage'));
ALTER TABLE identity.tenant_access_changes DROP CONSTRAINT ck_tenant_access_changes_action;
ALTER TABLE identity.tenant_access_changes ADD CONSTRAINT ck_tenant_access_changes_action
    CHECK (action IN ('metadata.read', 'metadata.manage', 'access.manage', 'protocol.manage'));
COMMENT ON COLUMN identity.tenant_action_grants.action IS
    'Exact tenant action. protocol.manage is separately delegated; metadata management never admits executable code.';

-- One immutable blob per tenant/content digest. Different package versions may
-- share exact bytes without exposing a global cross-tenant content oracle.
-- The catalogue's redundant unique key permits an admission foreign key to
-- prove both release identity and exact content identity, not just existence.
ALTER TABLE metadata.protocol_package_releases
    ADD CONSTRAINT uq_protocol_package_releases_content
    UNIQUE (tenant_id, protocol_package_id, protocol_package_version, artifact_sha256);
CREATE TABLE ingestion.protocol_artifacts (
    tenant_id TEXT NOT NULL,
    artifact_sha256 BYTEA NOT NULL,
    artifact_bytes BYTEA NOT NULL,
    CONSTRAINT pk_protocol_artifacts PRIMARY KEY (tenant_id, artifact_sha256),
    CONSTRAINT fk_protocol_artifacts_tenant FOREIGN KEY (tenant_id)
        REFERENCES metadata.tenants (tenant_id) ON DELETE RESTRICT,
    CONSTRAINT ck_protocol_artifacts_digest CHECK (octet_length(artifact_sha256) = 32),
    CONSTRAINT ck_protocol_artifacts_size CHECK (octet_length(artifact_bytes) BETWEEN 1 AND 8388608)
);
COMMENT ON TABLE ingestion.protocol_artifacts IS
    'Exact immutable untrusted component bytes, addressed by tenant and SHA-256. The host recomputes the digest and verifies release length on every retrieval; storage is not admission.';
COMMENT ON COLUMN ingestion.protocol_artifacts.artifact_sha256 IS
    'Host-computed binary SHA-256, shared by releases only inside the same tenant. Hash identity is not authenticity or execution authority.';
COMMENT ON COLUMN ingestion.protocol_artifacts.artifact_bytes IS
    'Bounded opaque component, initially at most 8 MiB; never rendered in routine diagnostics or returned by the catalogue API.';

-- Successful compilation evidence is keyed by world and reviewed policy. A
-- newer policy cannot inherit previous evidence. Failure/timeout creates no row.
-- First receipt actor/time are retained on retries, even from another operator.
CREATE TABLE ingestion.protocol_admissions (
    tenant_id TEXT NOT NULL,
    protocol_package_id TEXT NOT NULL,
    protocol_package_version TEXT NOT NULL,
    artifact_sha256 BYTEA NOT NULL,
    wit_world TEXT NOT NULL,
    policy_version INTEGER NOT NULL,
    actor_id TEXT NOT NULL,
    admitted_at_ms BIGINT NOT NULL,
    CONSTRAINT pk_protocol_admissions PRIMARY KEY (tenant_id, protocol_package_id, protocol_package_version, wit_world, policy_version),
    CONSTRAINT fk_protocol_admissions_artifact FOREIGN KEY (tenant_id, artifact_sha256)
        REFERENCES ingestion.protocol_artifacts (tenant_id, artifact_sha256) ON DELETE RESTRICT,
    CONSTRAINT fk_protocol_admissions_release FOREIGN KEY (tenant_id, protocol_package_id, protocol_package_version, artifact_sha256)
        REFERENCES metadata.protocol_package_releases (tenant_id, protocol_package_id, protocol_package_version, artifact_sha256) ON DELETE RESTRICT,
    CONSTRAINT fk_protocol_admissions_actor FOREIGN KEY (actor_id) REFERENCES identity.actors (actor_id) ON DELETE RESTRICT,
    CONSTRAINT ck_protocol_admissions_world CHECK (wit_world IN ('runtime-v1', 'identity-v1')),
    CONSTRAINT ck_protocol_admissions_policy CHECK (policy_version > 0),
    CONSTRAINT ck_protocol_admissions_time CHECK (admitted_at_ms >= 0)
);
COMMENT ON TABLE ingestion.protocol_admissions IS
    'Immutable successful host-policy/WIT admission evidence. This table grants no activation, device routing or session authority.';
COMMENT ON COLUMN ingestion.protocol_admissions.policy_version IS
    'Version of the reviewed byte, compiler isolation and WIT policy; changing those semantics requires a new version and fresh admission.';
COMMENT ON COLUMN ingestion.protocol_admissions.actor_id IS 'First authenticated operator with exact tenant protocol.manage; retries preserve attribution.';
COMMENT ON COLUMN ingestion.protocol_admissions.admitted_at_ms IS 'First successful host receipt time in Unix milliseconds, independent of upload time.';

-- Updates/deletes would invalidate exact retries and historical evidence. Future
-- retention must deliberately evolve this rule; no implicit GC is permitted.
CREATE FUNCTION ingestion.reject_protocol_artifact_mutation() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'protocol artifact and admission records are immutable' USING ERRCODE = '23514';
END;
$$;
COMMENT ON FUNCTION ingestion.reject_protocol_artifact_mutation() IS 'Reject mutation of protocol content/evidence; append-only lifecycle until reviewed retention exists.';
CREATE TRIGGER protocol_artifacts_immutable BEFORE UPDATE OR DELETE ON ingestion.protocol_artifacts
    FOR EACH ROW EXECUTE FUNCTION ingestion.reject_protocol_artifact_mutation();
CREATE TRIGGER protocol_admissions_immutable BEFORE UPDATE OR DELETE ON ingestion.protocol_admissions
    FOR EACH ROW EXECUTE FUNCTION ingestion.reject_protocol_artifact_mutation();
