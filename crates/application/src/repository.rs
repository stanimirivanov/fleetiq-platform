use std::{error::Error, future::Future};

use fleetiq_domain::{Asset, AssetId, TenantId};

/// Persists and retrieves asset aggregates within an explicit tenant boundary.
///
/// The associated error keeps infrastructure failures out of application and
/// domain types. Returning a future explicitly guarantees that implementations
/// remain usable from a multithreaded asynchronous runtime without requiring a
/// macro-generated trait transformation.
pub trait AssetRepository: Send + Sync {
    /// The infrastructure-specific failure returned by this repository.
    type Error: Error + Send + Sync + 'static;

    /// Inserts an asset and all of its external identifiers atomically.
    fn insert<'a>(
        &'a self,
        asset: &'a Asset,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a;

    /// Finds an asset by its tenant-scoped identity.
    fn find_by_id<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        asset_id: &'a AssetId,
    ) -> impl Future<Output = Result<Option<Asset>, Self::Error>> + Send + 'a;
}
