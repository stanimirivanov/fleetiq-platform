//! Restore selection state and consumer-owned bounded merge boundary.
use super::{
    PropertyHistoryPage, PropertyHistoryQuery, PropertyHistoryService, PropertyHistoryView,
};
use crate::{AssetPropertyChangeView, StoredPropertyChange};
use fleetiq_access::{AuthorizedTenant, ReadTelemetry};
use fleetiq_domain::TenantId;
use std::future::Future;
/// One database snapshot of the selected manifest/request, never interval completeness.
pub enum RestoredHistorySnapshot {
    /// Published archive exists but no restore has been requested.
    Archived,
    /// The selected restore is pending verification/publication.
    Restoring,
    /// Missing, replaced, cancelled, expired or invalid staging.
    Unavailable,
    /// One complete verified file, at most 2 MiB/sixteen interpretations.
    Ready(Vec<u8>),
}
/// Merges original evidence using the hot store's exact text/cursor ordering.
pub trait RestoredPropertyHistoryView: PropertyHistoryView {
    /// Reads bounded hot rows and combines at most sixteen verified restored rows.
    /// # Errors
    /// Rejects tenant/evidence disagreement and storage failures; never queries once per row.
    fn list_with_restored(
        &self,
        tenant: &TenantId,
        query: &PropertyHistoryQuery,
        restored: &[StoredPropertyChange],
    ) -> impl Future<Output = Result<PropertyHistoryPage, <Self as AssetPropertyChangeView>::Error>> + Send;
}
impl<V: RestoredPropertyHistoryView> PropertyHistoryService<V> {
    /// Reads a bounded union only under exact current telemetry authority.
    /// # Errors
    /// Preserves storage/evidence failures rather than returning complete empty history.
    pub async fn list_with_restored(
        &self,
        grant: AuthorizedTenant<ReadTelemetry>,
        query: &PropertyHistoryQuery,
        restored: &[StoredPropertyChange],
    ) -> Result<PropertyHistoryPage, V::Error> {
        self.view
            .list_with_restored(grant.tenant_id(), query, restored)
            .await
    }
}
