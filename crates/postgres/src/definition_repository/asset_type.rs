use fleetiq_domain::{
    AssetTypeDefinition, AssetTypeId, AssetTypeRef, DefinitionName, PropertyDefinitionId,
    PropertyDefinitionRef, TenantId,
};

use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};

use super::{PostgresDefinitionRepository, common::definition_version};

impl PostgresDefinitionRepository {
    /// Inserts an asset-type definition and its ordered properties atomically.
    ///
    /// A data-modifying common table expression inserts the definition and
    /// exposes its key to one set-based property insert. `UNNEST ... WITH
    /// ORDINALITY` preserves declaration order without issuing one database
    /// request per property. An empty property array still inserts the parent.
    pub(super) async fn insert_asset_type_definition(
        &self,
        tenant_id: &TenantId,
        definition: &AssetTypeDefinition,
    ) -> Result<(), PostgresRepositoryError> {
        let tenant_id = tenant_id.as_str();
        let type_id = definition.reference().id().as_str();
        let type_version = i64::from(definition.reference().version().get());
        let display_name = definition.name().as_str();
        let properties = definition.supported_properties();
        let property_count = i64::try_from(properties.len())
            .map_err(|error| corrupt("asset_type_definitions.property_count", error))?;
        let property_ids: Vec<_> = properties
            .iter()
            .map(|property| property.id().as_str())
            .collect();
        let property_versions: Vec<_> = properties
            .iter()
            .map(|property| i64::from(property.version().get()))
            .collect();

        sqlx::query!(
            r"
            WITH inserted_definition AS (
                INSERT INTO metadata.asset_type_definitions
                    (tenant_id, type_id, version, display_name, property_count)
                VALUES ($1, $2, $3, $4, $5)
                RETURNING tenant_id, type_id, version
            )
            INSERT INTO metadata.asset_type_properties
                (tenant_id, type_id, type_version, property_id,
                 property_version, property_ordinal)
            SELECT
                inserted_definition.tenant_id,
                inserted_definition.type_id,
                inserted_definition.version,
                property.property_id,
                property.property_version,
                property.property_ordinal
            FROM inserted_definition
            CROSS JOIN UNNEST($6::text[], $7::bigint[]) WITH ORDINALITY
                AS property(property_id, property_version, property_ordinal)
            ",
            tenant_id,
            type_id,
            type_version,
            display_name,
            property_count,
            &property_ids as &[&str],
            &property_versions as &[i64]
        )
        .execute(&self.pool)
        .await
        .map_err(classify_database_error)?;

        Ok(())
    }

    /// Reconstructs an asset-type definition with two intentional reads.
    ///
    /// Keeping the parent and ordered property rows separate preserves `SQLx`'s
    /// compile-time field mapping and clearly distinguishes a missing
    /// definition from one that declares no properties.
    pub(super) async fn find_asset_type_definition(
        &self,
        tenant_id: &TenantId,
        reference: &AssetTypeRef,
    ) -> Result<Option<AssetTypeDefinition>, PostgresRepositoryError> {
        let tenant_id = tenant_id.as_str();
        let type_id = reference.id().as_str();
        let type_version = i64::from(reference.version().get());
        let row = sqlx::query_as!(
            AssetTypeDefinitionRow,
            r"
            SELECT type_id, version, display_name
            FROM metadata.asset_type_definitions
            WHERE tenant_id = $1 AND type_id = $2 AND version = $3
            ",
            tenant_id,
            type_id,
            type_version
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(classify_database_error)?;

        let Some(row) = row else {
            return Ok(None);
        };

        let properties = sqlx::query_as!(
            PropertyReferenceRow,
            r"
            SELECT property_id, property_version
            FROM metadata.asset_type_properties
            WHERE tenant_id = $1 AND type_id = $2 AND type_version = $3
            ORDER BY property_ordinal
            ",
            tenant_id,
            type_id,
            type_version
        )
        .fetch_all(&self.pool)
        .await
        .map_err(classify_database_error)?;

        row.into_domain(properties).map(Some)
    }
}

#[derive(Debug)]
struct AssetTypeDefinitionRow {
    type_id: String,
    version: i64,
    display_name: String,
}

impl AssetTypeDefinitionRow {
    fn into_domain(
        self,
        properties: Vec<PropertyReferenceRow>,
    ) -> Result<AssetTypeDefinition, PostgresRepositoryError> {
        let mut definition = AssetTypeDefinition::new(
            AssetTypeRef::new(
                AssetTypeId::new(self.type_id)
                    .map_err(|error| corrupt("asset_type_definitions.type_id", error))?,
                definition_version("asset_type_definitions.version", self.version)?,
            ),
            DefinitionName::new(self.display_name)
                .map_err(|error| corrupt("asset_type_definitions.display_name", error))?,
        );

        for property in properties {
            definition
                .add_property(property.into_domain()?)
                .map_err(|error| corrupt("asset_type_properties", error))?;
        }

        Ok(definition)
    }
}

#[derive(Debug)]
struct PropertyReferenceRow {
    property_id: String,
    property_version: i64,
}

impl PropertyReferenceRow {
    fn into_domain(self) -> Result<PropertyDefinitionRef, PostgresRepositoryError> {
        Ok(PropertyDefinitionRef::new(
            PropertyDefinitionId::new(self.property_id)
                .map_err(|error| corrupt("asset_type_properties.property_id", error))?,
            definition_version(
                "asset_type_properties.property_version",
                self.property_version,
            )?,
        ))
    }
}
