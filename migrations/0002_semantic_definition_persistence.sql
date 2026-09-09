-- ============================================================================
-- Semantic definition persistence
-- ============================================================================
--
-- This migration completes the persisted semantic-definition catalogue used by
-- the current domain model:
--
--   1. Asset-type property order
--
--      Property membership was introduced in the metadata foundation. An
--      explicit ordinal now preserves the aggregate's declaration order during
--      a database round trip. A stored property count plus deferred constraint
--      triggers prevents properties from being appended after publication.
--
--   2. Relationship-type definitions
--
--      A relationship type gives a directed asset edge its stable meaning.
--      Relationships will reference one exact version in a later migration.
--
--   3. Definition immutability
--
--      Property, asset-type, and relationship-type versions are durable
--      semantic facts. Changing meaning requires a newly inserted version;
--      UPDATE and DELETE are rejected for existing versions and their property
--      membership rows.
--
-- The backfill order for existing asset-type properties is deterministic. It
-- does not claim to recover an order that was never stored; deployments that
-- contain pre-existing definitions must review that generated order before
-- applying this migration.
-- ============================================================================


-- ============================================================================
-- Asset-type property declaration order
-- ============================================================================

ALTER TABLE metadata.asset_type_properties
ADD COLUMN property_ordinal BIGINT;


-- Existing rows receive a stable order by property identity and version. New
-- writes provide the aggregate's original declaration order explicitly.
WITH ordered_properties AS (
    SELECT
        tenant_id,
        type_id,
        type_version,
        property_id,
        row_number() OVER (
            PARTITION BY tenant_id, type_id, type_version
            ORDER BY property_id, property_version
        ) AS property_ordinal
    FROM metadata.asset_type_properties
)
UPDATE metadata.asset_type_properties AS target
SET property_ordinal = ordered_properties.property_ordinal
FROM ordered_properties
WHERE target.tenant_id = ordered_properties.tenant_id
  AND target.type_id = ordered_properties.type_id
  AND target.type_version = ordered_properties.type_version
  AND target.property_id = ordered_properties.property_id;


ALTER TABLE metadata.asset_type_properties
ALTER COLUMN property_ordinal SET NOT NULL;


ALTER TABLE metadata.asset_type_properties
ADD CONSTRAINT ck_asset_type_properties_ordinal
CHECK (property_ordinal > 0);


-- Two properties cannot occupy the same position within one asset-type version.
ALTER TABLE metadata.asset_type_properties
ADD CONSTRAINT uq_asset_type_properties_ordinal
UNIQUE (tenant_id, type_id, type_version, property_ordinal);


-- ============================================================================
-- Asset-type property-set completeness
-- ============================================================================

-- property_count records the complete membership cardinality declared when the
-- immutable asset-type version is inserted. It lets PostgreSQL distinguish the
-- initial transactional composition from a later attempt to append meaning to
-- an already published version.
ALTER TABLE metadata.asset_type_definitions
ADD COLUMN property_count BIGINT;


UPDATE metadata.asset_type_definitions AS definition
SET property_count = (
    SELECT count(*)
    FROM metadata.asset_type_properties AS property
    WHERE property.tenant_id = definition.tenant_id
      AND property.type_id = definition.type_id
      AND property.type_version = definition.version
);


ALTER TABLE metadata.asset_type_definitions
ALTER COLUMN property_count SET NOT NULL;


ALTER TABLE metadata.asset_type_definitions
ADD CONSTRAINT ck_asset_type_definitions_property_count
CHECK (property_count >= 0);


-- Checks the declared property count after all statements in the transaction
-- have run. Deferral allows the repository to insert an asset-type row before
-- its property rows without exposing a partially valid committed definition.
CREATE FUNCTION metadata.check_asset_type_property_count()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    definition_tenant_id TEXT;
    definition_type_id TEXT;
    definition_version BIGINT;
    expected_count BIGINT;
    actual_count BIGINT;
BEGIN
    definition_tenant_id := COALESCE(NEW.tenant_id, OLD.tenant_id);
    definition_type_id := COALESCE(NEW.type_id, OLD.type_id);

    IF TG_TABLE_NAME = 'asset_type_definitions' THEN
        definition_version := COALESCE(NEW.version, OLD.version);
    ELSE
        definition_version := COALESCE(NEW.type_version, OLD.type_version);
    END IF;

    SELECT property_count
    INTO expected_count
    FROM metadata.asset_type_definitions
    WHERE tenant_id = definition_tenant_id
      AND type_id = definition_type_id
      AND version = definition_version;

    -- Parent deletion is governed by the definition immutability trigger below.
    -- A missing parent therefore needs no additional count check here.
    IF NOT FOUND THEN
        RETURN NULL;
    END IF;

    SELECT count(*)
    INTO actual_count
    FROM metadata.asset_type_properties
    WHERE tenant_id = definition_tenant_id
      AND type_id = definition_type_id
      AND type_version = definition_version;

    IF actual_count <> expected_count THEN
        RAISE EXCEPTION
            'Asset type %.% version % declares % properties but stores %',
            definition_tenant_id,
            definition_type_id,
            definition_version,
            expected_count,
            actual_count
            USING
                ERRCODE = '23514',
                CONSTRAINT = 'ck_asset_type_definition_property_membership';
    END IF;

    RETURN NULL;
END;
$$;


CREATE CONSTRAINT TRIGGER asset_type_definitions_check_property_count
AFTER INSERT OR UPDATE
ON metadata.asset_type_definitions
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION metadata.check_asset_type_property_count();


CREATE CONSTRAINT TRIGGER asset_type_properties_check_property_count
AFTER INSERT OR UPDATE OR DELETE
ON metadata.asset_type_properties
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION metadata.check_asset_type_property_count();


-- ============================================================================
-- Relationship-type definitions
-- ============================================================================

-- Each row defines one immutable version of a directed relationship's meaning.
--
-- Examples include `physical.contains`, `physical.tows`, and `monitored_by`.
-- Endpoint constraints and graph cardinality require access to other definitions
-- and relationships, so they remain application graph-validation concerns.
CREATE TABLE metadata.relationship_type_definitions (
    tenant_id TEXT NOT NULL,
    relationship_type_id TEXT NOT NULL,
    version BIGINT NOT NULL,
    display_name TEXT NOT NULL,

    CONSTRAINT pk_relationship_type_definitions
        PRIMARY KEY (tenant_id, relationship_type_id, version),

    CONSTRAINT fk_relationship_type_definitions_tenant
        FOREIGN KEY (tenant_id)
        REFERENCES metadata.tenants (tenant_id),

    CONSTRAINT ck_relationship_type_definitions_version
        CHECK (version BETWEEN 1 AND 4294967295),

    CONSTRAINT ck_relationship_type_definitions_id
        CHECK (metadata.is_valid_identifier(relationship_type_id)),

    CONSTRAINT ck_relationship_type_definitions_name
        CHECK (
            char_length(display_name) BETWEEN 1 AND 200
            AND display_name = btrim(display_name)
            AND display_name !~ '[[:cntrl:]]'
        )
);


-- ============================================================================
-- Database-level definition immutability
-- ============================================================================

-- Definition rows represent published semantic facts. The application appends
-- a new version rather than rewriting or removing meaning referenced by assets,
-- observations, relationships, or historical projections.
CREATE FUNCTION metadata.prevent_definition_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION
        'Table %.% contains immutable semantic definitions; insert a new version instead of using %',
        TG_TABLE_SCHEMA,
        TG_TABLE_NAME,
        TG_OP
        USING ERRCODE = '55000';
END;
$$;


CREATE TRIGGER property_definitions_prevent_mutation
BEFORE UPDATE OR DELETE
ON metadata.property_definitions
FOR EACH ROW
EXECUTE FUNCTION metadata.prevent_definition_mutation();


CREATE TRIGGER asset_type_definitions_prevent_mutation
BEFORE UPDATE OR DELETE
ON metadata.asset_type_definitions
FOR EACH ROW
EXECUTE FUNCTION metadata.prevent_definition_mutation();


CREATE TRIGGER asset_type_properties_prevent_mutation
BEFORE UPDATE OR DELETE
ON metadata.asset_type_properties
FOR EACH ROW
EXECUTE FUNCTION metadata.prevent_definition_mutation();


CREATE TRIGGER relationship_type_definitions_prevent_mutation
BEFORE UPDATE OR DELETE
ON metadata.relationship_type_definitions
FOR EACH ROW
EXECUTE FUNCTION metadata.prevent_definition_mutation();


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON COLUMN metadata.asset_type_properties.property_ordinal IS
    'One-based declaration order of a property within an exact asset-type version.';

COMMENT ON COLUMN metadata.asset_type_definitions.property_count IS
    'Complete number of property memberships declared by this immutable asset-type version.';

COMMENT ON FUNCTION metadata.check_asset_type_property_count() IS
    'Deferrably verifies that an asset-type version stores its complete declared property set.';

COMMENT ON TABLE metadata.relationship_type_definitions IS
    'Immutable versioned definitions of directed asset-relationship meaning.';

COMMENT ON COLUMN metadata.relationship_type_definitions.relationship_type_id IS
    'Stable semantic identity of a directed relationship type within one tenant.';

COMMENT ON COLUMN metadata.relationship_type_definitions.version IS
    'Positive semantic version represented by the Rust DefinitionVersion u32 type.';

COMMENT ON FUNCTION metadata.prevent_definition_mutation() IS
    'Rejects UPDATE and DELETE operations against published semantic-definition rows.';
