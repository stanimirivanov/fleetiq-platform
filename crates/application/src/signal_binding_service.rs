use std::{error::Error, fmt};

use fleetiq_domain::{RecordRevision, SignalBinding, SignalBindingId, SignalSource, TenantId};

use crate::{
    AuthorizedTenantAction, SignalBindingRepository, SignalBindingSnapshotQuery, TenantAction,
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
    /// Returns [`SignalBindingApplicationError::ActionRequired`] when the
    /// evidence authorizes another action,
    /// [`SignalBindingApplicationError::TenantMismatch`] when the revision is
    /// outside the authorized tenant, or wraps the repository error when the
    /// append fails.
    pub async fn append_revision(
        &self,
        authorization: AuthorizedTenantAction,
        binding: &SignalBinding,
    ) -> Result<(), SignalBindingApplicationError<R::Error>> {
        require_action(&authorization, TenantAction::ManageMetadata)?;

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
    /// Returns [`SignalBindingApplicationError::ActionRequired`] when the
    /// evidence authorizes another action, or wraps the repository error when
    /// retrieval fails.
    pub async fn find_revision(
        &self,
        authorization: AuthorizedTenantAction,
        binding_id: &SignalBindingId,
        revision: RecordRevision,
    ) -> Result<Option<SignalBinding>, SignalBindingApplicationError<R::Error>> {
        require_action(&authorization, TenantAction::ReadMetadata)?;

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
    /// Returns [`SignalBindingApplicationError::ActionRequired`] when the
    /// evidence authorizes another action, or wraps the repository error when
    /// retrieval fails.
    pub async fn find_for_source(
        &self,
        authorization: AuthorizedTenantAction,
        source: &SignalSource,
        query: SignalBindingSnapshotQuery,
    ) -> Result<Vec<SignalBinding>, SignalBindingApplicationError<R::Error>> {
        require_action(&authorization, TenantAction::ReadMetadata)?;

        self.repository
            .find_for_source(authorization.tenant_id(), source, query)
            .await
            .map_err(SignalBindingApplicationError::Repository)
    }
}

/// Failure to execute an authorized signal-binding operation.
#[derive(Debug)]
pub enum SignalBindingApplicationError<E> {
    /// The supplied evidence authorizes a different tenant action.
    ActionRequired {
        /// Action required by the attempted operation.
        required: TenantAction,
        /// Action carried by the rejected authorization evidence.
        actual: TenantAction,
    },
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
            Self::ActionRequired { .. } => {
                formatter.write_str("signal-binding operation requires different authorization")
            }
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
            Self::ActionRequired { .. } | Self::TenantMismatch { .. } => None,
        }
    }
}

fn require_action<E>(
    authorization: &AuthorizedTenantAction,
    required: TenantAction,
) -> Result<(), SignalBindingApplicationError<E>> {
    let actual = authorization.action();
    if actual == required {
        Ok(())
    } else {
        Err(SignalBindingApplicationError::ActionRequired { required, actual })
    }
}

#[cfg(test)]
mod tests;
