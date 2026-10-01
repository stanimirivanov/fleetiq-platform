use fleetiq_domain::{
    DefinitionName, PropertyDefinition, PropertyDefinitionId, PropertyDefinitionRef,
    PropertyValueKind, TenantId, UnitId,
};

use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};

use super::{PostgresDefinitionRepository, common::definition_version};

impl PostgresDefinitionRepository {
    /// Inserts one immutable property-definition row.
    pub(super) async fn insert_property_definition(
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

    /// Reads and validates one exact property-definition version.
    pub(super) async fn find_property_definition(
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
