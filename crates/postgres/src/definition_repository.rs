use std::future::Future;

use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};
use fleetiq_application::DefinitionRepository;
use fleetiq_domain::{
    AssetTypeDefinition, AssetTypeId, AssetTypeRef, DefinitionName, DefinitionVersion,
    PropertyDefinition, PropertyDefinitionId, PropertyDefinitionRef, PropertyValueKind,
    RelationshipTypeDefinition, RelationshipTypeId, RelationshipTypeRef, TenantId, UnitId,
};
use sqlx::PgPool;

/// PostgreSQL implementation of the semantic-definition repository port.
#[derive(Clone, Debug)]
pub struct PostgresDefinitionRepository {
    pool: PgPool,
}

impl PostgresDefinitionRepository {
    /// Constructs a definition repository using an existing connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn insert_property_definition(
        &self,
        tenant_id: &TenantId,
        definition: &PropertyDefinition,
    ) -> Result<(), PostgresRepositoryError> {
        let tenant_id = tenant_id.as_str();
        let property_id = definition.id().as_str();
        let version = i64::from(definition.version().get());
        let display_name = definition.name().as_str();
        let value_kind = value_kind_as_str(definition.value_kind());
        let canonical_unit = definition.canonical_unit().map(AsRef::as_ref);

        sqlx::query!(
            r"
            INSERT INTO metadata.property_definitions
                (tenant_id, property_id, version, display_name, value_kind, canonical_unit)
            VALUES ($1, $2, $3, $4, $5, $6)
            ",
            tenant_id,
            property_id,
            version,
            display_name,
            value_kind,
            canonical_unit
        )
        .execute(&self.pool)
        .await
        .map_err(classify_database_error)?;

        Ok(())
    }

    async fn find_property_definition(
        &self,
        tenant_id: &TenantId,
        reference: &PropertyDefinitionRef,
    ) -> Result<Option<PropertyDefinition>, PostgresRepositoryError> {
        let row = sqlx::query_as!(
            PropertyDefinitionRow,
            r"
            SELECT property_id, version, display_name, value_kind, canonical_unit
            FROM metadata.property_definitions
            WHERE tenant_id = $1 AND property_id = $2 AND version = $3
            ",
            tenant_id.as_str(),
            reference.id().as_str(),
            i64::from(reference.version().get())
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(classify_database_error)?;

        row.map(PropertyDefinitionRow::into_domain).transpose()
    }

    async fn insert_asset_type_definition(
        &self,
        tenant_id: &TenantId,
        definition: &AssetTypeDefinition,
    ) -> Result<(), PostgresRepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(classify_database_error)?;
        let tenant_id = tenant_id.as_str();
        let type_id = definition.reference().id().as_str();
        let type_version = i64::from(definition.reference().version().get());
        let display_name = definition.name().as_str();
        let property_count = i64::try_from(definition.supported_properties().len())
            .map_err(|error| corrupt("asset_type_definitions.property_count", error))?;

        sqlx::query!(
            r"
            INSERT INTO metadata.asset_type_definitions
                (tenant_id, type_id, version, display_name, property_count)
            VALUES ($1, $2, $3, $4, $5)
            ",
            tenant_id,
            type_id,
            type_version,
            display_name,
            property_count
        )
        .execute(&mut *transaction)
        .await
        .map_err(classify_database_error)?;

        for (index, property) in definition.supported_properties().iter().enumerate() {
            let property_ordinal = i64::try_from(index + 1)
                .map_err(|error| corrupt("asset_type_properties.property_ordinal", error))?;
            let property_id = property.id().as_str();
            let property_version = i64::from(property.version().get());

            sqlx::query!(
                r"
                INSERT INTO metadata.asset_type_properties
                    (tenant_id, type_id, type_version, property_id,
                     property_version, property_ordinal)
                VALUES ($1, $2, $3, $4, $5, $6)
                ",
                tenant_id,
                type_id,
                type_version,
                property_id,
                property_version,
                property_ordinal
            )
            .execute(&mut *transaction)
            .await
            .map_err(classify_database_error)?;
        }

        transaction.commit().await.map_err(classify_database_error)
    }

    async fn find_asset_type_definition(
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

    async fn insert_relationship_type_definition(
        &self,
        tenant_id: &TenantId,
        definition: &RelationshipTypeDefinition,
    ) -> Result<(), PostgresRepositoryError> {
        let reference = definition.reference();

        sqlx::query!(
            r"
            INSERT INTO metadata.relationship_type_definitions
                (tenant_id, relationship_type_id, version, display_name)
            VALUES ($1, $2, $3, $4)
            ",
            tenant_id.as_str(),
            reference.id().as_str(),
            i64::from(reference.version().get()),
            definition.name().as_str()
        )
        .execute(&self.pool)
        .await
        .map_err(classify_database_error)?;

        Ok(())
    }

    async fn find_relationship_type_definition(
        &self,
        tenant_id: &TenantId,
        reference: &RelationshipTypeRef,
    ) -> Result<Option<RelationshipTypeDefinition>, PostgresRepositoryError> {
        let row = sqlx::query_as!(
            RelationshipTypeDefinitionRow,
            r"
            SELECT relationship_type_id, version, display_name
            FROM metadata.relationship_type_definitions
            WHERE tenant_id = $1 AND relationship_type_id = $2 AND version = $3
            ",
            tenant_id.as_str(),
            reference.id().as_str(),
            i64::from(reference.version().get())
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(classify_database_error)?;

        row.map(RelationshipTypeDefinitionRow::into_domain)
            .transpose()
    }
}

impl DefinitionRepository for PostgresDefinitionRepository {
    type Error = PostgresRepositoryError;

    fn insert_property<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        definition: &'a PropertyDefinition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        self.insert_property_definition(tenant_id, definition)
    }

    fn find_property<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a PropertyDefinitionRef,
    ) -> impl Future<Output = Result<Option<PropertyDefinition>, Self::Error>> + Send + 'a {
        self.find_property_definition(tenant_id, reference)
    }

    fn insert_asset_type<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        definition: &'a AssetTypeDefinition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        self.insert_asset_type_definition(tenant_id, definition)
    }

    fn find_asset_type<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a AssetTypeRef,
    ) -> impl Future<Output = Result<Option<AssetTypeDefinition>, Self::Error>> + Send + 'a {
        self.find_asset_type_definition(tenant_id, reference)
    }

    fn insert_relationship_type<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        definition: &'a RelationshipTypeDefinition,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        self.insert_relationship_type_definition(tenant_id, definition)
    }

    fn find_relationship_type<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a RelationshipTypeRef,
    ) -> impl Future<Output = Result<Option<RelationshipTypeDefinition>, Self::Error>> + Send + 'a
    {
        self.find_relationship_type_definition(tenant_id, reference)
    }
}

#[derive(Debug)]
struct PropertyDefinitionRow {
    property_id: String,
    version: i64,
    display_name: String,
    value_kind: String,
    canonical_unit: Option<String>,
}

impl PropertyDefinitionRow {
    fn into_domain(self) -> Result<PropertyDefinition, PostgresRepositoryError> {
        PropertyDefinition::new(
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new(self.property_id)
                    .map_err(|error| corrupt("property_definitions.property_id", error))?,
                definition_version("property_definitions.version", self.version)?,
            ),
            DefinitionName::new(self.display_name)
                .map_err(|error| corrupt("property_definitions.display_name", error))?,
            parse_value_kind(&self.value_kind)?,
            self.canonical_unit
                .map(UnitId::new)
                .transpose()
                .map_err(|error| corrupt("property_definitions.canonical_unit", error))?,
        )
        .map_err(|error| corrupt("property_definitions", error))
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

#[derive(Debug)]
struct RelationshipTypeDefinitionRow {
    relationship_type_id: String,
    version: i64,
    display_name: String,
}

impl RelationshipTypeDefinitionRow {
    fn into_domain(self) -> Result<RelationshipTypeDefinition, PostgresRepositoryError> {
        Ok(RelationshipTypeDefinition::new(
            RelationshipTypeRef::new(
                RelationshipTypeId::new(self.relationship_type_id).map_err(|error| {
                    corrupt("relationship_type_definitions.relationship_type_id", error)
                })?,
                definition_version("relationship_type_definitions.version", self.version)?,
            ),
            DefinitionName::new(self.display_name)
                .map_err(|error| corrupt("relationship_type_definitions.display_name", error))?,
        ))
    }
}

const fn value_kind_as_str(value_kind: PropertyValueKind) -> &'static str {
    match value_kind {
        PropertyValueKind::Boolean => "boolean",
        PropertyValueKind::Integer => "integer",
        PropertyValueKind::Decimal => "decimal",
        PropertyValueKind::Text => "text",
    }
}

fn parse_value_kind(value: &str) -> Result<PropertyValueKind, PostgresRepositoryError> {
    match value {
        "boolean" => Ok(PropertyValueKind::Boolean),
        "integer" => Ok(PropertyValueKind::Integer),
        "decimal" => Ok(PropertyValueKind::Decimal),
        "text" => Ok(PropertyValueKind::Text),
        value => Err(corrupt(
            "property_definitions.value_kind",
            format_args!("unsupported value kind {value:?}"),
        )),
    }
}

fn definition_version(
    field: &'static str,
    value: i64,
) -> Result<DefinitionVersion, PostgresRepositoryError> {
    let value = u32::try_from(value).map_err(|error| corrupt(field, error))?;
    DefinitionVersion::new(value).map_err(|error| corrupt(field, error))
}
