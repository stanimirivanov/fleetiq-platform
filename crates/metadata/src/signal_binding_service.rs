use std::{error::Error, fmt};

use fleetiq_domain::{
    AssetId, RecordRevision, SignalBinding, SignalBindingId, SignalSource, TenantId,
};

use crate::{
    AuthorizedTenant, ManageMetadata, ReadMetadata, SignalBindingRepository,
    SignalBindingSnapshotPage, SignalBindingSnapshotPageQuery, SignalBindingSnapshotQuery,
};

/// Coordinates authorized operations on signal-to-property attributions.
///
/// The service consumes authorization evidence for every operation. Reads
/// derive tenant scope from that evidence, while revision appends additionally
/// verify that the supplied binding belongs to the authorized tenant before
/// invoking persistence.
#[derive(Clone, Debug)]
pub struct SignalBindingApplicationService<R> {
    repository: R,
}

impl<R> SignalBindingApplicationService<R> {
    /// Creates the service using the supplied signal-binding repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> SignalBindingApplicationService<R>
where
    R: SignalBindingRepository,
{
    /// Appends one immutable binding revision with `ManageMetadata` authorization.
    ///
    /// Authorization evidence is consumed so it cannot be reused for another
    /// operation. A binding owned by another tenant is rejected before the
    /// repository is called.
    ///
    /// # Errors
    ///
    /// Returns [`SignalBindingApplicationError::TenantMismatch`] when the
    /// revision is outside the authorized tenant, or wraps the repository
    /// error when the append fails.
    pub async fn append_revision(
        &self,
        authorization: AuthorizedTenant<ManageMetadata>,
        binding: &SignalBinding,
    ) -> Result<(), SignalBindingApplicationError<R::Error>> {
        if authorization.tenant_id() != binding.tenant_id() {
            return Err(SignalBindingApplicationError::TenantMismatch {
                authorized_tenant_id: authorization.tenant_id().clone(),
                binding_tenant_id: binding.tenant_id().clone(),
            });
        }

        self.repository
            .insert_revision(binding)
            .await
            .map_err(SignalBindingApplicationError::Repository)
    }

    /// Finds one exact recorded binding revision with `ReadMetadata` authorization.
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
        binding_id: &SignalBindingId,
        revision: RecordRevision,
    ) -> Result<Option<SignalBinding>, SignalBindingApplicationError<R::Error>> {
        self.repository
            .find_revision(authorization.tenant_id(), binding_id, revision)
            .await
            .map_err(SignalBindingApplicationError::Repository)
    }

    /// Resolves bindings for a decoded source in a bitemporal snapshot.
    ///
    /// Tenant scope is derived exclusively from consumed `ReadMetadata`
    /// authorization. The query retains its independent physical-time and
    /// recorded-time cutoffs.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when retrieval fails.
    pub async fn find_for_source(
        &self,
        authorization: AuthorizedTenant<ReadMetadata>,
        source: &SignalSource,
        query: SignalBindingSnapshotQuery,
    ) -> Result<Vec<SignalBinding>, SignalBindingApplicationError<R::Error>> {
        self.repository
            .find_for_source(authorization.tenant_id(), source, query)
            .await
            .map_err(SignalBindingApplicationError::Repository)
    }

    /// Reads one bounded source-attribution snapshot page under tenant authorization.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when retrieval fails.
    pub async fn find_for_source_page(
        &self,
        authorization: AuthorizedTenant<ReadMetadata>,
        source: &SignalSource,
        query: &SignalBindingSnapshotPageQuery,
    ) -> Result<SignalBindingSnapshotPage, SignalBindingApplicationError<R::Error>> {
        self.repository
            .find_for_source_page(authorization.tenant_id(), source, query)
            .await
            .map_err(SignalBindingApplicationError::Repository)
    }

    /// Reads a bounded target-asset attribution page under tenant authorization.
    ///
    /// `None` means the requested asset does not exist in the authorized
    /// tenant. An empty page means it exists without matching bindings.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when retrieval fails.
    pub async fn find_for_target_page(
        &self,
        authorization: AuthorizedTenant<ReadMetadata>,
        asset_id: &AssetId,
        query: &SignalBindingSnapshotPageQuery,
    ) -> Result<Option<SignalBindingSnapshotPage>, SignalBindingApplicationError<R::Error>> {
        self.repository
            .find_for_target_page(authorization.tenant_id(), asset_id, query)
            .await
            .map_err(SignalBindingApplicationError::Repository)
    }
}

/// Failure to execute an authorized signal-binding operation.
#[derive(Debug)]
pub enum SignalBindingApplicationError<E> {
    /// The signal-binding revision belongs to another tenant.
    TenantMismatch {
        /// Tenant carried by the authorization evidence.
        authorized_tenant_id: TenantId,
        /// Tenant that owns the rejected signal-binding revision.
        binding_tenant_id: TenantId,
    },
    /// The persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for SignalBindingApplicationError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TenantMismatch { .. } => {
                formatter.write_str("signal binding is outside the authorized tenant")
            }
            Self::Repository(_) => {
                formatter.write_str("signal-binding repository operation failed")
            }
        }
    }
}

impl<E> Error for SignalBindingApplicationError<E>
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
