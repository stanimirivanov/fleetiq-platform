-- ============================================================================
-- FleetIQ metadata foundation
-- ============================================================================
--
-- This migration introduces the relational foundation for three related
-- concepts:
--
--   1. Tenant boundaries
--
--      Every semantic definition and asset belongs to one tenant. Tenant-aware
--      composite keys prevent references and uniqueness rules from crossing
--      that boundary.
--
--   2. Versioned semantic definitions
--
--      Property and asset-type definitions use an exact positive version.
--      Assets therefore retain the meaning assigned to them even after a later
--      definition version is introduced.
--
--   3. Asset identity
--
--      An asset has a FleetIQ-internal identity and can have several typed
--      identifiers assigned by external authorities. External identifiers are
--      attributes of the asset aggregate and are written in the same
--      transaction as the asset.
--
-- Internal and semantic identifiers are stored as case-sensitive text. Their
-- database checks intentionally mirror the domain's common identifier rules:
-- non-empty, no surrounding whitespace, no control characters, and at most 128
-- Unicode characters.
--
-- This migration does not introduce telemetry, device endpoints, semantic
-- signal bindings, or asset relationships. Those concepts depend on this
-- metadata foundation and will be added through focused migrations.
-- ============================================================================


-- ============================================================================
-- Metadata namespace and shared validation
-- ============================================================================

-- Product metadata is isolated from PostgreSQL's public namespace so ownership,
-- privileges, migrations, and operational inspection can address it explicitly.
CREATE SCHEMA metadata;


-- Returns whether text satisfies the common identifier representation used by
-- the initial domain model.
--
-- This function centralizes the rule so identifier-bearing columns do not drift
-- as new metadata tables are added. It validates representation only; each
-- identifier newtype still gives the value its domain-specific meaning.
CREATE FUNCTION metadata.is_valid_identifier(value TEXT)
RETURNS BOOLEAN
LANGUAGE SQL
IMMUTABLE
PARALLEL SAFE
RETURN char_length(value) BETWEEN 1 AND 128
    AND value = btrim(value)
    AND value !~ '[[:cntrl:]]';


-- ============================================================================
-- Tenants
-- ============================================================================

-- A tenant is the ownership and isolation boundary for FleetIQ metadata.
--
-- The application supplies the identifier. Tenant lifecycle attributes and
-- authorization policy are deliberately deferred until their requirements are
-- defined.
CREATE TABLE metadata.tenants (
    tenant_id TEXT PRIMARY KEY,

    CONSTRAINT ck_tenants_id
        CHECK (metadata.is_valid_identifier(tenant_id))
);


-- ============================================================================
-- Semantic property definitions
-- ============================================================================

-- Each row defines one semantic version of an observable property.
--
-- Examples of property identities include `location.latitude` and
-- `freezer.air.temperature`. Version is part of the identity so a consumer can
-- pin the exact meaning used when data was interpreted.
CREATE TABLE metadata.property_definitions (
    tenant_id TEXT NOT NULL,
    property_id TEXT NOT NULL,
    version BIGINT NOT NULL,
    display_name TEXT NOT NULL,

    -- value_kind is a closed initial set understood by domain validation and
    -- future telemetry normalization. New kinds require a coordinated schema
    -- and domain-model change.
    value_kind TEXT NOT NULL,

    -- canonical_unit identifies the normalized storage and analytics unit for a
    -- numeric property. NULL means that the property is unitless or non-numeric.
    canonical_unit TEXT,

    CONSTRAINT pk_property_definitions
        PRIMARY KEY (tenant_id, property_id, version),

    CONSTRAINT fk_property_definitions_tenant
        FOREIGN KEY (tenant_id)
        REFERENCES metadata.tenants (tenant_id),

    -- DefinitionVersion is a non-zero u32 in the Rust domain. BIGINT avoids a
    -- signed PostgreSQL representation rejecting valid upper-half u32 values.
    CONSTRAINT ck_property_definitions_version
        CHECK (version BETWEEN 1 AND 4294967295),

    CONSTRAINT ck_property_definitions_value_kind
        CHECK (value_kind IN ('boolean', 'integer', 'decimal', 'text')),

    CONSTRAINT ck_property_definitions_unit
        CHECK (canonical_unit IS NULL OR value_kind IN ('integer', 'decimal')),

    CONSTRAINT ck_property_definitions_id
        CHECK (metadata.is_valid_identifier(property_id)),

    CONSTRAINT ck_property_definitions_unit_id
        CHECK (
            canonical_unit IS NULL
            OR metadata.is_valid_identifier(canonical_unit)
        ),

    CONSTRAINT ck_property_definitions_name
        CHECK (
            char_length(display_name) BETWEEN 1 AND 200
            AND display_name = btrim(display_name)
            AND display_name !~ '[[:cntrl:]]'
        )
);


-- ============================================================================
-- Semantic asset-type definitions
-- ============================================================================

-- Each row names one exact version of an asset type within a tenant.
--
-- Types describe capabilities rather than an industry-specific class hierarchy.
-- Examples can include a generic machine, a vehicle, a refrigeration subsystem,
-- or another physical or logical asset introduced by a tenant.
CREATE TABLE metadata.asset_type_definitions (
    tenant_id TEXT NOT NULL,
    type_id TEXT NOT NULL,
    version BIGINT NOT NULL,
    display_name TEXT NOT NULL,

    CONSTRAINT pk_asset_type_definitions
        PRIMARY KEY (tenant_id, type_id, version),

    CONSTRAINT fk_asset_type_definitions_tenant
        FOREIGN KEY (tenant_id)
        REFERENCES metadata.tenants (tenant_id),

    CONSTRAINT ck_asset_type_definitions_version
        CHECK (version BETWEEN 1 AND 4294967295),

    CONSTRAINT ck_asset_type_definitions_id
        CHECK (metadata.is_valid_identifier(type_id)),

    CONSTRAINT ck_asset_type_definitions_name
        CHECK (
            char_length(display_name) BETWEEN 1 AND 200
            AND display_name = btrim(display_name)
            AND display_name !~ '[[:cntrl:]]'
        )
);


-- Associates an exact asset-type version with the exact property-definition
-- versions it supports.
--
-- The primary key allows a property identity only once in an asset-type version.
-- Changing the selected property version therefore requires a new asset-type
-- version rather than two competing meanings for the same property.
CREATE TABLE metadata.asset_type_properties (
    tenant_id TEXT NOT NULL,
    type_id TEXT NOT NULL,
    type_version BIGINT NOT NULL,
    property_id TEXT NOT NULL,
    property_version BIGINT NOT NULL,

    CONSTRAINT pk_asset_type_properties
        PRIMARY KEY (tenant_id, type_id, type_version, property_id),

    -- Including tenant_id in both foreign keys makes cross-tenant semantic
    -- composition impossible even if textual identifiers happen to match.
    CONSTRAINT fk_asset_type_properties_asset_type
        FOREIGN KEY (tenant_id, type_id, type_version)
        REFERENCES metadata.asset_type_definitions (tenant_id, type_id, version),

    CONSTRAINT fk_asset_type_properties_property
        FOREIGN KEY (tenant_id, property_id, property_version)
        REFERENCES metadata.property_definitions (tenant_id, property_id, version)
);


-- ============================================================================
-- Assets
-- ============================================================================

-- An asset is an industry-neutral entity that FleetIQ monitors or manages.
--
-- asset_id is the stable FleetIQ-internal identity within a tenant. type_id and
-- type_version pin the asset to the exact semantic definition used to interpret
-- it. A later definition version does not silently change an existing asset.
CREATE TABLE metadata.assets (
    tenant_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    type_id TEXT NOT NULL,
    type_version BIGINT NOT NULL,
    display_name TEXT NOT NULL,

    CONSTRAINT pk_assets
        PRIMARY KEY (tenant_id, asset_id),

    CONSTRAINT fk_assets_tenant
        FOREIGN KEY (tenant_id)
        REFERENCES metadata.tenants (tenant_id),

    -- The composite reference guarantees that the type belongs to the asset's
    -- tenant and that the requested definition version exists.
    CONSTRAINT fk_assets_type
        FOREIGN KEY (tenant_id, type_id, type_version)
        REFERENCES metadata.asset_type_definitions (tenant_id, type_id, version),

    CONSTRAINT ck_assets_id
        CHECK (metadata.is_valid_identifier(asset_id)),

    CONSTRAINT ck_assets_name
        CHECK (
            char_length(display_name) BETWEEN 1 AND 200
            AND display_name = btrim(display_name)
            AND display_name !~ '[[:cntrl:]]'
        )
);


-- Stores identifiers assigned to an asset by a device ecosystem, manufacturer,
-- operator, registry, or another external authority.
--
-- Examples include an IMEI, serial number, registration number, or a customer's
-- existing equipment identifier. These values do not replace the internal
-- asset_id because their format, authority, and lifecycle are external to
-- FleetIQ.
CREATE TABLE metadata.asset_external_identifiers (
    tenant_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    identifier_kind TEXT NOT NULL,

    -- authority disambiguates the issuer or namespace where the kind alone is
    -- insufficient. NULL represents a kind with one implicit authority.
    authority TEXT,

    identifier_value TEXT NOT NULL,

    -- External identifiers are owned by the asset aggregate. Removing an asset
    -- removes these dependent rows in the same database operation.
    CONSTRAINT fk_asset_external_identifiers_asset
        FOREIGN KEY (tenant_id, asset_id)
        REFERENCES metadata.assets (tenant_id, asset_id)
        ON DELETE CASCADE,

    CONSTRAINT ck_asset_external_identifiers_kind
        CHECK (metadata.is_valid_identifier(identifier_kind)),

    CONSTRAINT ck_asset_external_identifiers_authority
        CHECK (
            authority IS NULL
            OR metadata.is_valid_identifier(authority)
        ),

    CONSTRAINT ck_asset_external_identifiers_value
        CHECK (metadata.is_valid_identifier(identifier_value)),

    -- NULLS NOT DISTINCT makes an absent authority one real namespace for
    -- uniqueness purposes. Two assets in the same tenant cannot claim the same
    -- kind/value pair merely because both authority values are NULL.
    CONSTRAINT uq_asset_external_identifier
        UNIQUE NULLS NOT DISTINCT (
            tenant_id,
            identifier_kind,
            authority,
            identifier_value
        )
);


-- Supports aggregate reconstruction without scanning identifiers belonging to
-- other assets in the tenant. The uniqueness constraint above starts with a
-- different column order and does not efficiently serve this access path.
CREATE INDEX ix_asset_external_identifiers_asset
ON metadata.asset_external_identifiers (
    tenant_id,
    asset_id
);


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON SCHEMA metadata IS
    'Tenant-owned semantic definitions and asset metadata for FleetIQ.';

COMMENT ON FUNCTION metadata.is_valid_identifier(TEXT) IS
    'Checks the shared textual representation required by FleetIQ identifier newtypes.';

COMMENT ON TABLE metadata.tenants IS
    'Stable ownership and isolation boundaries for FleetIQ metadata.';

COMMENT ON TABLE metadata.property_definitions IS
    'Versioned semantic definitions of observable asset properties.';

COMMENT ON TABLE metadata.asset_type_definitions IS
    'Versioned, industry-neutral definitions of asset capabilities and meaning.';

COMMENT ON TABLE metadata.asset_type_properties IS
    'Exact property-definition versions supported by an exact asset-type version.';

COMMENT ON TABLE metadata.assets IS
    'Tenant-owned assets pinned to an exact semantic asset-type definition.';

COMMENT ON TABLE metadata.asset_external_identifiers IS
    'Typed identifiers assigned to an asset by authorities outside FleetIQ.';

COMMENT ON COLUMN metadata.property_definitions.version IS
    'Positive semantic version represented by the Rust DefinitionVersion u32 type.';

COMMENT ON COLUMN metadata.property_definitions.value_kind IS
    'Closed domain value kind used to validate and interpret normalized observations.';

COMMENT ON COLUMN metadata.property_definitions.canonical_unit IS
    'Normalized unit identifier for numeric observations; NULL for unitless or non-numeric properties.';

COMMENT ON COLUMN metadata.asset_type_properties.property_version IS
    'Exact property-definition version selected by this asset-type version.';

COMMENT ON COLUMN metadata.assets.asset_id IS
    'Stable FleetIQ-internal asset identity within one tenant.';

COMMENT ON COLUMN metadata.assets.type_version IS
    'Exact asset-type definition version used to interpret this asset.';

COMMENT ON COLUMN metadata.asset_external_identifiers.identifier_kind IS
    'Identifier scheme, such as imei or manufacturer.serial-number.';

COMMENT ON COLUMN metadata.asset_external_identifiers.authority IS
    'Issuer or namespace of the external identifier; NULL means one implicit authority.';

COMMENT ON COLUMN metadata.asset_external_identifiers.identifier_value IS
    'Case-sensitive identifier value as assigned by the external authority.';
