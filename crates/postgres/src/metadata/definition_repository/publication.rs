//! Resolve only the aggregate primary-key collision; all other failures stay errors.
use super::PostgresDefinitionRepository;
use crate::{PostgresRepositoryError, error::corrupt};
use fleetiq_domain::TenantId;
use fleetiq_metadata::{
    DefinitionPublicationOutcome as Outcome, DefinitionPublicationRepository, SemanticDefinition,
};

impl DefinitionPublicationRepository for PostgresDefinitionRepository {
    type Error = PostgresRepositoryError;

    /// Reuses the strict atomic insert algorithm without weakening its callers.
    ///
    /// A duplicate primary key waits for the winning transaction to finish.
    /// The subsequent fresh READ COMMITTED query sees that complete immutable
    /// aggregate. Never compare in the failed statement's original snapshot or
    /// recover from an unrelated constraint. No failed insert leaves child rows.
    async fn publish(
        &self,
        tenant: &TenantId,
        definition: &SemanticDefinition,
    ) -> Result<Outcome, Self::Error> {
        let (attempt, primary_key) = match definition {
            SemanticDefinition::Property(value) => (
                self.insert_property_definition(tenant, value).await,
                "pk_property_definitions",
            ),
            SemanticDefinition::AssetType(value) => (
                self.insert_asset_type_definition(tenant, value).await,
                "pk_asset_type_definitions",
            ),
            SemanticDefinition::RelationshipType(value) => (
                self.insert_relationship_type_definition(tenant, value)
                    .await,
                "pk_relationship_type_definitions",
            ),
        };
        match attempt {
            Ok(()) => return Ok(Outcome::Published),
            Err(PostgresRepositoryError::Conflict {
                constraint: Some(ref name),
                ..
            }) if name == primary_key => {}
            Err(error) => return Err(error),
        }
        let stored = match definition {
            SemanticDefinition::Property(value) => self
                .find_property_definition(tenant, value.reference())
                .await?
                .map(SemanticDefinition::Property),
            SemanticDefinition::AssetType(value) => self
                .find_asset_type_definition(tenant, value.reference())
                .await?
                .map(SemanticDefinition::AssetType),
            SemanticDefinition::RelationshipType(value) => self
                .find_relationship_type_definition(tenant, value.reference())
                .await?
                .map(SemanticDefinition::RelationshipType),
        }
        .ok_or_else(|| {
            corrupt(
                "metadata.definition_publication",
                "primary-key winner is missing",
            )
        })?;
        Ok(if stored == *definition {
            Outcome::AlreadyPublished
        } else {
            Outcome::Conflict
        })
    }
}
