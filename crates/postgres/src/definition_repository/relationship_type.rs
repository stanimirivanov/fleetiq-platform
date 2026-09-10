use fleetiq_domain::{
    DefinitionName, RelationshipTypeDefinition, RelationshipTypeId, RelationshipTypeRef, TenantId,
};

use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};

use super::{PostgresDefinitionRepository, common::definition_version};

impl PostgresDefinitionRepository {
    /// Inserts one immutable relationship-type definition row.
    pub(super) async fn insert_relationship_type_definition(
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

    /// Reads and validates one exact relationship-type definition version.
    pub(super) async fn find_relationship_type_definition(
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
