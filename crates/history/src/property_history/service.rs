//! Consumer-owned read port and typed telemetry authorization boundary.
use super::{PropertyHistoryPage, PropertyHistoryQuery};
use crate::{AssetPropertyChangeView, StoredPropertyChange};
use fleetiq_access::{AuthorizedTenant, ReadTelemetry};
use fleetiq_domain::{SignalTarget, TenantId};
use fleetiq_semantics::AssetPropertyChangeId;
use std::future::Future;

/// Set-based hot-history port; implementations must not query once per entry.
pub trait PropertyHistoryView: AssetPropertyChangeView {
    /// Finds at most one page for the tenant and exact target, plus a look-ahead.
    ///
    /// # Errors
    /// Returns infrastructure or persisted-data reconstruction errors, never an
    /// empty success for unavailable storage. Missing targets yield an empty page.
    fn list_property<'a>(
        &'a self,
        tenant: &'a TenantId,
        query: &'a PropertyHistoryQuery,
    ) -> impl Future<Output = Result<PropertyHistoryPage, <Self as AssetPropertyChangeView>::Error>>
    + Send
    + 'a;
}
/// Protected history use cases; metadata authority cannot be substituted.
pub struct PropertyHistoryService<V> {
    view: V,
}
impl<V> PropertyHistoryService<V> {
    /// Composes the consumer-owned read adapter.
    pub const fn new(view: V) -> Self {
        Self { view }
    }
}
impl<V: PropertyHistoryView> PropertyHistoryService<V> {
    /// Returns a bounded page only after exact telemetry authorization.
    ///
    /// # Errors
    /// Preserves the adapter failure; callers must render a safe transport error.
    pub async fn list(
        &self,
        grant: AuthorizedTenant<ReadTelemetry>,
        query: &PropertyHistoryQuery,
    ) -> Result<PropertyHistoryPage, V::Error> {
        self.view.list_property(grant.tenant_id(), query).await
    }
    /// Reads original source and attribution, denying target/tenant fallback.
    ///
    /// # Errors
    /// Preserves repository failures. An absent interpretation or another target
    /// returns absence, without disclosing where the interpretation belongs.
    pub async fn find(
        &self,
        grant: AuthorizedTenant<ReadTelemetry>,
        target: &SignalTarget,
        id: &AssetPropertyChangeId,
    ) -> Result<Option<StoredPropertyChange>, V::Error> {
        Ok(self
            .view
            .find(grant.tenant_id(), id)
            .await?
            .filter(|c| c.target() == target))
    }
}
