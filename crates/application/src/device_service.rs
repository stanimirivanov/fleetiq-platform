use std::{error::Error, fmt};

use fleetiq_domain::{AssetId, CommunicationDevice, TenantId};

use crate::{AuthorizedTenantAction, DeviceRepository, TenantAction};

/// Coordinates authorized communication-device operations.
///
/// The service consumes authorization evidence for every operation. Reads
/// derive tenant scope from that evidence, while registration additionally
/// verifies that the supplied device belongs to the authorized tenant before
/// invoking persistence.
#[derive(Clone, Debug)]
pub struct DeviceApplicationService<R> {
    repository: R,
}

impl<R> DeviceApplicationService<R> {
    /// Creates the service using the supplied device repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> DeviceApplicationService<R>
where
    R: DeviceRepository,
{
    /// Registers communication capability using exact `ManageMetadata` authorization.
    ///
    /// The aggregate contains the complete ordered endpoint declaration. An
    /// aggregate owned by another tenant is rejected before the repository is
    /// called.
    ///
    /// # Errors
    ///
    /// Returns [`DeviceApplicationError::ActionRequired`] when the evidence
    /// authorizes another action, [`DeviceApplicationError::TenantMismatch`]
    /// when the device is outside the authorized tenant, or wraps the
    /// repository error when registration fails.
    pub async fn register(
        &self,
        authorization: AuthorizedTenantAction,
        device: &CommunicationDevice,
    ) -> Result<(), DeviceApplicationError<R::Error>> {
        require_action(&authorization, TenantAction::ManageMetadata)?;

        if authorization.tenant_id() != device.tenant_id() {
            return Err(DeviceApplicationError::TenantMismatch {
                authorized_tenant_id: authorization.tenant_id().clone(),
                device_tenant_id: device.tenant_id().clone(),
            });
        }

        self.repository
            .insert(device)
            .await
            .map_err(DeviceApplicationError::Repository)
    }

    /// Finds communication capability for one asset with `ReadMetadata` authorization.
    ///
    /// Tenant scope is derived exclusively from the consumed authorization
    /// evidence; the caller supplies only the underlying asset identity.
    ///
    /// # Errors
    ///
    /// Returns [`DeviceApplicationError::ActionRequired`] when the evidence
    /// authorizes another action, or wraps the repository error when retrieval
    /// fails.
    pub async fn find_by_asset(
        &self,
        authorization: AuthorizedTenantAction,
        asset_id: &AssetId,
    ) -> Result<Option<CommunicationDevice>, DeviceApplicationError<R::Error>> {
        require_action(&authorization, TenantAction::ReadMetadata)?;

        self.repository
            .find_by_asset(authorization.tenant_id(), asset_id)
            .await
            .map_err(DeviceApplicationError::Repository)
    }
}

/// Failure to execute an authorized communication-device operation.
#[derive(Debug)]
pub enum DeviceApplicationError<E> {
    /// The supplied evidence authorizes a different tenant action.
    ActionRequired {
        /// Action required by the attempted operation.
        required: TenantAction,
        /// Action carried by the rejected authorization evidence.
        actual: TenantAction,
    },
    /// The device aggregate belongs to another tenant.
    TenantMismatch {
        /// Tenant carried by the authorization evidence.
        authorized_tenant_id: TenantId,
        /// Tenant that owns the rejected communication device.
        device_tenant_id: TenantId,
    },
    /// The persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for DeviceApplicationError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ActionRequired { .. } => {
                formatter.write_str("device operation requires different authorization")
            }
            Self::TenantMismatch { .. } => {
                formatter.write_str("device is outside the authorized tenant")
            }
            Self::Repository(_) => formatter.write_str("device repository operation failed"),
        }
    }
}

impl<E> Error for DeviceApplicationError<E>
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
) -> Result<(), DeviceApplicationError<E>> {
    let actual = authorization.action();
    if actual == required {
        Ok(())
    } else {
        Err(DeviceApplicationError::ActionRequired { required, actual })
    }
}

#[cfg(test)]
mod tests;
