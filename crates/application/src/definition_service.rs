use std::{error::Error, fmt};

use fleetiq_domain::{
    AssetTypeDefinition, AssetTypeRef, PropertyDefinition, PropertyDefinitionRef,
    RelationshipTypeDefinition, RelationshipTypeRef,
};

use crate::{AuthorizedTenantAction, DefinitionRepository, TenantAction};

/// Coordinates authorized publication and retrieval of semantic definitions.
///
/// Definition aggregates do not carry tenant identity because they describe
/// reusable semantic meaning. This service supplies repository tenant scope
/// exclusively from consumed authorization evidence so callers cannot publish
/// or read a definition in a separately selected tenant.
#[derive(Clone, Debug)]
pub struct DefinitionApplicationService<R> {
    repository: R,
}

impl<R> DefinitionApplicationService<R> {
    /// Creates the service using the supplied definition repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> DefinitionApplicationService<R>
where
    R: DefinitionRepository,
{
    /// Publishes one immutable property-definition version.
    ///
    /// # Errors
    ///
    /// Returns [`DefinitionApplicationError::ActionRequired`] unless the
    /// evidence authorizes `ManageMetadata`, or wraps the repository error when
    /// publication fails.
    pub async fn publish_property(
        &self,
        authorization: AuthorizedTenantAction,
        definition: &PropertyDefinition,
    ) -> Result<(), DefinitionApplicationError<R::Error>> {
        require_action(&authorization, TenantAction::ManageMetadata)?;
        self.repository
            .insert_property(authorization.tenant_id(), definition)
            .await
            .map_err(DefinitionApplicationError::Repository)
    }

    /// Finds one exact property-definition version.
    ///
    /// # Errors
    ///
    /// Returns [`DefinitionApplicationError::ActionRequired`] unless the
    /// evidence authorizes `ReadMetadata`, or wraps the repository error when
    /// retrieval fails.
    pub async fn find_property(
        &self,
        authorization: AuthorizedTenantAction,
        reference: &PropertyDefinitionRef,
    ) -> Result<Option<PropertyDefinition>, DefinitionApplicationError<R::Error>> {
        require_action(&authorization, TenantAction::ReadMetadata)?;
        self.repository
            .find_property(authorization.tenant_id(), reference)
            .await
            .map_err(DefinitionApplicationError::Repository)
    }

    /// Publishes one immutable asset-type-definition version.
    ///
    /// # Errors
    ///
    /// Returns [`DefinitionApplicationError::ActionRequired`] unless the
    /// evidence authorizes `ManageMetadata`, or wraps the repository error when
    /// publication fails.
    pub async fn publish_asset_type(
        &self,
        authorization: AuthorizedTenantAction,
        definition: &AssetTypeDefinition,
    ) -> Result<(), DefinitionApplicationError<R::Error>> {
        require_action(&authorization, TenantAction::ManageMetadata)?;
        self.repository
            .insert_asset_type(authorization.tenant_id(), definition)
            .await
            .map_err(DefinitionApplicationError::Repository)
    }

    /// Finds one exact asset-type-definition version.
    ///
    /// # Errors
    ///
    /// Returns [`DefinitionApplicationError::ActionRequired`] unless the
    /// evidence authorizes `ReadMetadata`, or wraps the repository error when
    /// retrieval fails.
    pub async fn find_asset_type(
        &self,
        authorization: AuthorizedTenantAction,
        reference: &AssetTypeRef,
    ) -> Result<Option<AssetTypeDefinition>, DefinitionApplicationError<R::Error>> {
        require_action(&authorization, TenantAction::ReadMetadata)?;
        self.repository
            .find_asset_type(authorization.tenant_id(), reference)
            .await
            .map_err(DefinitionApplicationError::Repository)
    }

    /// Publishes one immutable relationship-type-definition version.
    ///
    /// # Errors
    ///
    /// Returns [`DefinitionApplicationError::ActionRequired`] unless the
    /// evidence authorizes `ManageMetadata`, or wraps the repository error when
    /// publication fails.
    pub async fn publish_relationship_type(
        &self,
        authorization: AuthorizedTenantAction,
        definition: &RelationshipTypeDefinition,
    ) -> Result<(), DefinitionApplicationError<R::Error>> {
        require_action(&authorization, TenantAction::ManageMetadata)?;
        self.repository
            .insert_relationship_type(authorization.tenant_id(), definition)
            .await
            .map_err(DefinitionApplicationError::Repository)
    }

    /// Finds one exact relationship-type-definition version.
    ///
    /// # Errors
    ///
    /// Returns [`DefinitionApplicationError::ActionRequired`] unless the
    /// evidence authorizes `ReadMetadata`, or wraps the repository error when
    /// retrieval fails.
    pub async fn find_relationship_type(
        &self,
        authorization: AuthorizedTenantAction,
        reference: &RelationshipTypeRef,
    ) -> Result<Option<RelationshipTypeDefinition>, DefinitionApplicationError<R::Error>> {
        require_action(&authorization, TenantAction::ReadMetadata)?;
        self.repository
            .find_relationship_type(authorization.tenant_id(), reference)
            .await
            .map_err(DefinitionApplicationError::Repository)
    }
}

/// Failure to execute an authorized semantic-definition operation.
#[derive(Debug)]
pub enum DefinitionApplicationError<E> {
    /// The supplied evidence authorizes a different tenant action.
    ActionRequired {
        /// Action required by the attempted operation.
        required: TenantAction,
        /// Action carried by the rejected authorization evidence.
        actual: TenantAction,
    },
    /// The persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for DefinitionApplicationError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ActionRequired { .. } => {
                formatter.write_str("definition operation requires different authorization")
            }
            Self::Repository(_) => formatter.write_str("definition repository operation failed"),
        }
    }
}

impl<E> Error for DefinitionApplicationError<E>
where
    E: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(error) => Some(error),
            Self::ActionRequired { .. } => None,
        }
    }
}

fn require_action<E>(
    authorization: &AuthorizedTenantAction,
    required: TenantAction,
) -> Result<(), DefinitionApplicationError<E>> {
    let actual = authorization.action();
    if actual == required {
        Ok(())
    } else {
        Err(DefinitionApplicationError::ActionRequired { required, actual })
    }
}

#[cfg(test)]
mod tests;
