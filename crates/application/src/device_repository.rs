use std::{error::Error, future::Future};

use fleetiq_domain::{AssetId, CommunicationDevice, TenantId};

/// Persists communication capability attached to tenant-owned assets.
///
/// Device identity is the identity of its existing asset. Endpoints are owned
/// by the device aggregate and are persisted atomically with its registration.
pub trait DeviceRepository: Send + Sync {
    /// The infrastructure-specific failure returned by this repository.
    type Error: Error + Send + Sync + 'static;

    /// Registers one asset as a communication device with its endpoints.
    fn insert<'a>(
        &'a self,
        device: &'a CommunicationDevice,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a;

    /// Finds communication capability for one asset within a tenant.
    fn find_by_asset<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        asset_id: &'a AssetId,
    ) -> impl Future<Output = Result<Option<CommunicationDevice>, Self::Error>> + Send + 'a;
}
