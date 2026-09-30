use std::{error::Error, fmt};

use fleetiq_domain::{AssetId, AssetRelationship, AssetRelationshipId, RecordRevision, TenantId};

use crate::{
    AuthorizedTenant, ManageMetadata, ReadMetadata, RelationshipRepository,
    RelationshipSnapshotQuery,
};

/// Coordinates authorized operations on a tenant's semantic asset graph.
///
/// The service consumes authorization evidence for every operation. Reads
/// derive tenant scope from that evidence, while revision appends additionally
/// verify that the supplied relationship belongs to the authorized tenant
/// before invoking persistence.
#[derive(Clone, Debug)]
pub struct RelationshipApplicationService<R> {
    repository: R,
}

impl<R> RelationshipApplicationService<R> {
    /// Creates the service using the supplied relationship repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> RelationshipApplicationService<R>
where
    R: RelationshipRepository,
{
    /// Appends one immutable relationship revision with `ManageMetadata` authorization.
    ///
    /// Authorization evidence is consumed so it cannot be reused for another
    /// operation. A relationship owned by another tenant is rejected before
    /// the repository is called.
    ///
    /// # Errors
    ///
    /// Returns [`RelationshipApplicationError::TenantMismatch`] when the
    /// revision is outside the authorized tenant, or wraps the repository
    /// error when the append fails.
    pub async fn append_revision(
        &self,
        authorization: AuthorizedTenant<ManageMetadata>,
        relationship: &AssetRelationship,
    ) -> Result<(), RelationshipApplicationError<R::Error>> {
        if authorization.tenant_id() != relationship.tenant_id() {
            return Err(RelationshipApplicationError::TenantMismatch {
                authorized_tenant_id: authorization.tenant_id().clone(),
                relationship_tenant_id: relationship.tenant_id().clone(),
            });
        }

        self.repository
            .insert_revision(relationship)
            .await
            .map_err(RelationshipApplicationError::Repository)
    }

    /// Finds one exact recorded relationship revision with `ReadMetadata` authorization.
    ///
    /// Tenant scope is derived exclusively from the consumed authorization
    /// evidence.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when retrieval fails.
    pub async fn find_revision(
        &self,
        authorization: AuthorizedTenant<ReadMetadata>,
        relationship_id: &AssetRelationshipId,
        revision: RecordRevision,
    ) -> Result<Option<AssetRelationship>, RelationshipApplicationError<R::Error>> {
        self.repository
            .find_revision(authorization.tenant_id(), relationship_id, revision)
            .await
            .map_err(RelationshipApplicationError::Repository)
    }

    /// Finds relationships involving an asset in a bitemporal snapshot.
    ///
    /// Tenant scope is derived exclusively from consumed `ReadMetadata`
    /// authorization. The query retains its independent physical-time and
    /// recorded-time cutoffs.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when retrieval fails.
    pub async fn find_for_asset(
        &self,
        authorization: AuthorizedTenant<ReadMetadata>,
        asset_id: &AssetId,
        query: RelationshipSnapshotQuery,
    ) -> Result<Vec<AssetRelationship>, RelationshipApplicationError<R::Error>> {
        self.repository
            .find_for_asset(authorization.tenant_id(), asset_id, query)
            .await
            .map_err(RelationshipApplicationError::Repository)
    }
}

/// Failure to execute an authorized relationship operation.
#[derive(Debug)]
pub enum RelationshipApplicationError<E> {
    /// The relationship revision belongs to another tenant.
    TenantMismatch {
        /// Tenant carried by the authorization evidence.
        authorized_tenant_id: TenantId,
        /// Tenant that owns the rejected relationship revision.
        relationship_tenant_id: TenantId,
    },
    /// The persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for RelationshipApplicationError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TenantMismatch { .. } => {
                formatter.write_str("relationship is outside the authorized tenant")
            }
            Self::Repository(_) => formatter.write_str("relationship repository operation failed"),
        }
    }
}

impl<E> Error for RelationshipApplicationError<E>
where
    E: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(error) => Some(error),
            Self::TenantMismatch { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests;
