-- ============================================================================
-- Immutable protocol package release catalogue
-- ============================================================================
--
-- This migration records the stable content identity of protocol package
-- releases before artifact storage, runtime admission, or activation is added.
-- A release key is scoped by tenant so independent customers can use the same
-- package ID and version without sharing a namespace or observing each other.
--
-- Each key permanently names one SHA-256 digest and exact raw artifact byte
-- length. Package versions are opaque labels rather than content hashes. The
-- database therefore rejects updates and deletes instead of allowing artifact
-- replacement beneath a release used by replay or audit evidence.
--
-- Catalogue presence records content metadata only. It does not prove that the
-- bytes exist in object storage, passed WebAssembly admission, have a trusted
-- signature, or may be activated for a tenant or device. Those concepts have
-- different lifecycles and require later explicit schemas.
--
-- The table is new and empty, so this transactional migration requires no
-- backfill or compatibility window. Its primary key and tenant foreign key take
-- only normal creation locks on the new relation.
-- ============================================================================


-- ============================================================================
-- Protocol package releases
-- ============================================================================

CREATE TABLE metadata.protocol_package_releases (
    tenant_id TEXT NOT NULL,
    protocol_package_id TEXT NOT NULL,
    protocol_package_version TEXT NOT NULL,

    -- Store the digest in its native fixed-width binary form. Text encodings are
    -- presentation concerns and could otherwise admit mixed case or prefixes.
    artifact_sha256 BYTEA NOT NULL,

    -- An unconstrained NUMERIC plus explicit checks preserves the complete u64
    -- application range exactly. BIGINT cannot represent values above i64::MAX;
    -- NUMERIC(20, 0) would silently round fractional input before a check sees it.
    artifact_size_bytes NUMERIC NOT NULL,

    CONSTRAINT pk_protocol_package_releases
        PRIMARY KEY (
            tenant_id,
            protocol_package_id,
            protocol_package_version
        ),

    CONSTRAINT fk_protocol_package_releases_tenant
        FOREIGN KEY (tenant_id)
        REFERENCES metadata.tenants (tenant_id)
        ON DELETE NO ACTION,

    CONSTRAINT ck_protocol_package_releases_package_id
        CHECK (metadata.is_valid_identifier(protocol_package_id)),

    CONSTRAINT ck_protocol_package_releases_package_version
        CHECK (metadata.is_valid_identifier(protocol_package_version)),

    CONSTRAINT ck_protocol_package_releases_artifact_sha256
        CHECK (octet_length(artifact_sha256) = 32),

    CONSTRAINT ck_protocol_package_releases_artifact_size
        CHECK (
            scale(artifact_size_bytes) = 0
            AND artifact_size_bytes BETWEEN 1 AND 18446744073709551615
        )
);


-- ============================================================================
-- Append-only enforcement
-- ============================================================================

CREATE FUNCTION metadata.prevent_protocol_package_release_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION
        'Protocol package releases are immutable; % is not permitted on %.%',
        TG_OP,
        TG_TABLE_SCHEMA,
        TG_TABLE_NAME;
END;
$$;

CREATE TRIGGER protocol_package_releases_prevent_mutation
BEFORE UPDATE OR DELETE ON metadata.protocol_package_releases
FOR EACH ROW
EXECUTE FUNCTION metadata.prevent_protocol_package_release_mutation();


-- ============================================================================
-- Database catalogue documentation
-- ============================================================================

COMMENT ON TABLE metadata.protocol_package_releases IS
    'Tenant-scoped immutable content metadata for protocol package releases; registration does not imply admission or activation.';

COMMENT ON COLUMN metadata.protocol_package_releases.tenant_id IS
    'Tenant that owns the package namespace and release metadata.';

COMMENT ON COLUMN metadata.protocol_package_releases.protocol_package_id IS
    'Stable package identity within the owning tenant.';

COMMENT ON COLUMN metadata.protocol_package_releases.protocol_package_version IS
    'Opaque immutable release label within a tenant and package identity.';

COMMENT ON COLUMN metadata.protocol_package_releases.artifact_sha256 IS
    'Exact 32-byte SHA-256 digest of the raw component artifact.';

COMMENT ON COLUMN metadata.protocol_package_releases.artifact_size_bytes IS
    'Positive integral raw artifact byte length, represented across the full unsigned 64-bit range.';

COMMENT ON FUNCTION metadata.prevent_protocol_package_release_mutation() IS
    'Rejects UPDATE and DELETE operations against immutable protocol package releases.';
