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

/// A bounded, exclusive-keyset request for one asset's relationship snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationshipSnapshotPageQuery {
    snapshot: RelationshipSnapshotQuery,
    limit: u8,
    after: Option<AssetRelationshipId>,
}

impl RelationshipSnapshotPageQuery {
    /// Maximum number of relationships returned by one page.
    pub const MAX_LIMIT: u8 = 100;

    /// Creates a page request. The limit must be between 1 and 100.
    ///
    /// # Errors
    ///
    /// Returns an error for an unbounded or empty page.
    pub fn new(
        snapshot: RelationshipSnapshotQuery,
        limit: u8,
        after: Option<AssetRelationshipId>,
    ) -> Result<Self, InvalidRelationshipPageLimit> {
        if limit == 0 || limit > Self::MAX_LIMIT {
            return Err(InvalidRelationshipPageLimit);
        }
        Ok(Self {
            snapshot,
            limit,
            after,
        })
    }

    /// Returns the two explicit snapshot cutoffs.
    pub const fn snapshot(&self) -> RelationshipSnapshotQuery {
        self.snapshot
    }

    /// Returns the maximum result count.
    pub const fn limit(&self) -> u8 {
        self.limit
    }

    /// Returns the exclusive relationship identity cursor.
    pub fn after(&self) -> Option<&AssetRelationshipId> {
        self.after.as_ref()
    }
}

/// A page limit outside the supported range of 1 through 100.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidRelationshipPageLimit;

impl std::fmt::Display for InvalidRelationshipPageLimit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("relationship page limit must be between 1 and 100")
    }
}

impl Error for InvalidRelationshipPageLimit {}

/// One ordered page of relationship revisions for an existing asset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationshipSnapshotPage {
    relationships: Vec<AssetRelationship>,
    next_after: Option<AssetRelationshipId>,
}

impl RelationshipSnapshotPage {
    /// Creates a page assembled by a repository adapter.
    pub fn new(
        relationships: Vec<AssetRelationship>,
        next_after: Option<AssetRelationshipId>,
    ) -> Self {
        Self {
            relationships,
            next_after,
        }
    }

    /// Returns relationship revisions in ascending stable-identity order.
    pub fn relationships(&self) -> &[AssetRelationship] {
        &self.relationships
    }

    /// Returns the exclusive cursor for the next page, when one exists.
    pub fn next_after(&self) -> Option<&AssetRelationshipId> {
        self.next_after.as_ref()
    }
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

    /// Returns a bounded snapshot page, or `None` if the asset does not exist.
    ///
    /// Implementations must choose the latest revision known at the recorded
    /// cutoff before filtering its current endpoints and effective interval.
    fn find_for_asset_page(
        &self,
        tenant_id: &TenantId,
        asset_id: &AssetId,
        query: &RelationshipSnapshotPageQuery,
    ) -> impl Future<Output = Result<Option<RelationshipSnapshotPage>, Self::Error>> + Send;
}
