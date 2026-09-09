use std::{error::Error, future::Future};

use fleetiq_domain::{
    AssetId, AssetRelationship, AssetRelationshipId, RecordRevision, TenantId, UnixTimeMillis,
};

/// Selects the relationship facts visible at two independent time dimensions.
///
/// `effective_at` describes when the relationship applied in the physical
/// world. `known_at` describes the inclusive cutoff for facts recorded by
/// `FleetIQ`. Keeping both values explicit prevents delayed corrections from
/// being interpreted through today's graph accidentally.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RelationshipSnapshotQuery {
    effective_at: UnixTimeMillis,
    known_at: UnixTimeMillis,
}

impl RelationshipSnapshotQuery {
    /// Constructs a bitemporal relationship snapshot query.
    pub const fn new(effective_at: UnixTimeMillis, known_at: UnixTimeMillis) -> Self {
        Self {
            effective_at,
            known_at,
        }
    }

    /// Returns the physical-time instant selected by this query.
    pub const fn effective_at(self) -> UnixTimeMillis {
        self.effective_at
    }

    /// Returns the inclusive recorded-time cutoff selected by this query.
    pub const fn known_at(self) -> UnixTimeMillis {
        self.known_at
    }
}

/// Persists and queries revisioned relationships within a tenant asset graph.
///
/// Every inserted value is a complete immutable revision. Implementations must
/// serialize concurrent changes to one stable relationship identity and reject
/// gaps in its recorded revision sequence.
pub trait RelationshipRepository: Send + Sync {
    /// The infrastructure-specific failure returned by this repository.
    type Error: Error + Send + Sync + 'static;

    /// Appends one immutable relationship revision.
    fn insert_revision(
        &self,
        relationship: &AssetRelationship,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Finds one exact recorded revision within a tenant.
    fn find_revision(
        &self,
        tenant_id: &TenantId,
        relationship_id: &AssetRelationshipId,
        revision: RecordRevision,
    ) -> impl Future<Output = Result<Option<AssetRelationship>, Self::Error>> + Send;

    /// Finds relationships involving an asset in a bitemporal snapshot.
    ///
    /// The result contains the latest revision of each relationship recorded by
    /// `query.known_at()` when that revision involves `asset_id` and is effective
    /// at `query.effective_at()`.
    fn find_for_asset(
        &self,
        tenant_id: &TenantId,
        asset_id: &AssetId,
        query: RelationshipSnapshotQuery,
    ) -> impl Future<Output = Result<Vec<AssetRelationship>, Self::Error>> + Send;
}
