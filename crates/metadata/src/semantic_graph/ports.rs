//! Consumer-owned consistent-read session; no database or transport types enter metadata.
use super::SemanticGraphQuery;
use crate::AssetCatalogueEntry;
use fleetiq_domain::{
    AssetId, AssetRelationship, AssetRelationshipId, SignalBinding, SignalBindingId, TenantId,
};
use std::{error::Error, future::Future};

/// Opens one immutable database view for every layer of an exploration.
pub trait SemanticGraphRepository: Send + Sync {
    /// Storage failure; public transport must not render its internal details.
    type Error: Error + Send + Sync + 'static;
    /// A tenant/cutoff-bound read session, rolled back or closed on cancellation.
    type Read: SemanticGraphRead<Error = Self::Error>;
    /// Establishes one consistent snapshot; no independently timed pool reads.
    fn open(
        &self,
        tenant: &TenantId,
        query: &SemanticGraphQuery,
    ) -> impl Future<Output = Result<Self::Read, Self::Error>> + Send;
}
/// Bounded set-wise storage reads consumed only by the graph coordinator.
///
/// Asset/frontier sets contain at most 100 IDs, excluded link sets at most 200.
/// Each link limit is 1–201 including a private look-ahead. Every call shares
/// tenant, database snapshot and temporal cutoffs established by `open`.
pub trait SemanticGraphRead: Send {
    /// Failure to read or reconstruct immutable facts.
    type Error: Error + Send + Sync + 'static;
    /// Loads bounded asset summaries set-wise in stable ID order.
    fn assets(
        &mut self,
        ids: &[AssetId],
    ) -> impl Future<Output = Result<Vec<AssetCatalogueEntry>, Self::Error>> + Send;
    /// Loads a C-ordered prefix of latest-known, effective relationship adjacency.
    /// Select latest revisions before testing endpoints; exclude included IDs.
    fn relationships(
        &mut self,
        frontier: &[AssetId],
        excluded: &[AssetRelationshipId],
        limit: u16,
    ) -> impl Future<Output = Result<Vec<AssetRelationship>, Self::Error>> + Send;
    /// Loads a C-ordered prefix of latest-known, effective binding adjacency.
    /// A binding connects its source device asset to its target asset.
    fn bindings(
        &mut self,
        frontier: &[AssetId],
        excluded: &[SignalBindingId],
        limit: u16,
    ) -> impl Future<Output = Result<Vec<SignalBinding>, Self::Error>> + Send;
}
