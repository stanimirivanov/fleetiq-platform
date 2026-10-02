use std::{error::Error, future::Future};

use fleetiq_domain::{Asset, AssetId, AssetName, AssetTypeRef, TenantId};

/// A bounded catalogue projection; exact asset reads provide identifiers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetCatalogueEntry {
    tenant_id: TenantId,
    id: AssetId,
    name: AssetName,
    asset_type: AssetTypeRef,
}

impl AssetCatalogueEntry {
    /// Creates a projection from already validated domain values.
    pub fn new(
        tenant_id: TenantId,
        id: AssetId,
        name: AssetName,
        asset_type: AssetTypeRef,
    ) -> Self {
        Self {
            tenant_id,
            id,
            name,
            asset_type,
        }
    }

    /// Returns the owning tenant.
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the stable asset identity.
    pub const fn id(&self) -> &AssetId {
        &self.id
    }

    /// Returns the operator-facing asset name.
    pub const fn name(&self) -> &AssetName {
        &self.name
    }

    /// Returns the pinned type version.
    pub const fn asset_type(&self) -> &AssetTypeRef {
        &self.asset_type
    }
}

/// An exclusive-keyset request for a bounded tenant asset catalogue page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetPageQuery {
    limit: u8,
    after: Option<AssetId>,
}

impl AssetPageQuery {
    /// Maximum number of assets returned by one request.
    pub const MAX_LIMIT: u8 = 100;

    /// Creates a request for 1 through 100 assets in ascending ID order.
    ///
    /// # Errors
    ///
    /// Returns an error if `limit` is outside the supported range.
    pub fn new(limit: u8, after: Option<AssetId>) -> Result<Self, InvalidAssetPageLimit> {
        if limit == 0 || limit > Self::MAX_LIMIT {
            return Err(InvalidAssetPageLimit);
        }
        Ok(Self { limit, after })
    }

    /// Returns the maximum number of assets in the page.
    pub const fn limit(&self) -> u8 {
        self.limit
    }

    /// Returns the exclusive stable-ID cursor, if provided.
    pub fn after(&self) -> Option<&AssetId> {
        self.after.as_ref()
    }
}

/// An asset catalogue page limit outside 1 through 100.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidAssetPageLimit;

impl std::fmt::Display for InvalidAssetPageLimit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("asset page limit must be between 1 and 100")
    }
}

impl Error for InvalidAssetPageLimit {}

/// One ordered page of bounded tenant-owned asset summaries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetPage {
    assets: Vec<AssetCatalogueEntry>,
    next_after: Option<AssetId>,
}

impl AssetPage {
    /// Creates a page assembled by a repository adapter.
    pub fn new(assets: Vec<AssetCatalogueEntry>, next_after: Option<AssetId>) -> Self {
        Self { assets, next_after }
    }

    /// Returns assets in ascending stable-ID order.
    pub fn assets(&self) -> &[AssetCatalogueEntry] {
        &self.assets
    }

    /// Returns the exclusive cursor for the next page, if one exists.
    pub fn next_after(&self) -> Option<&AssetId> {
        self.next_after.as_ref()
    }
}

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

    /// Lists one bounded tenant page of asset summaries in stable-ID order.
    /// External identifiers belong to the exact asset read, whose aggregate
    /// child count is not currently bounded.
    fn list_page<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        query: &'a AssetPageQuery,
    ) -> impl Future<Output = Result<AssetPage, Self::Error>> + Send + 'a;
}
