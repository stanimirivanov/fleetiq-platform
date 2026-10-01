use std::future::Future;

use fleetiq_domain::{AssetId, AssetRelationship, AssetRelationshipId, RecordRevision, TenantId};
use fleetiq_metadata::{RelationshipRepository, RelationshipSnapshotQuery};
use sqlx::PgPool;

use crate::error::PostgresRepositoryError;

mod commands;
mod common;
mod queries;
mod row;

/// PostgreSQL implementation of the revisioned relationship repository port.
#[derive(Clone, Debug)]
pub struct PostgresRelationshipRepository {
    pool: PgPool,
}

impl PostgresRelationshipRepository {
    /// Constructs a relationship repository using an existing connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl RelationshipRepository for PostgresRelationshipRepository {
    type Error = PostgresRepositoryError;

    fn insert_revision(
        &self,
        relationship: &AssetRelationship,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.append_revision(relationship)
    }

    fn find_revision(
        &self,
        tenant_id: &TenantId,
        relationship_id: &AssetRelationshipId,
        revision: RecordRevision,
    ) -> impl Future<Output = Result<Option<AssetRelationship>, Self::Error>> + Send {
        self.find_exact_revision(tenant_id, relationship_id, revision)
    }

    fn find_for_asset(
        &self,
        tenant_id: &TenantId,
        asset_id: &AssetId,
        query: RelationshipSnapshotQuery,
    ) -> impl Future<Output = Result<Vec<AssetRelationship>, Self::Error>> + Send {
        self.find_asset_snapshot(tenant_id, asset_id, query)
    }
}
