use bigdecimal::BigDecimal;
use fleetiq_domain::{AssetId, AssetRelationship, AssetRelationshipId, RecordRevision, TenantId};
use fleetiq_metadata::{
    RelationshipSnapshotPage, RelationshipSnapshotPageQuery, RelationshipSnapshotQuery,
};

use crate::error::{PostgresRepositoryError, classify_database_error};

use super::{PostgresRelationshipRepository, row::RelationshipRevisionRow};

impl PostgresRelationshipRepository {
    /// Retrieves at most `limit + 1` latest-known relationships in keyset order.
    ///
    /// A separate existence check distinguishes an empty graph from an unknown
    /// asset. The relationship query still selects a latest known revision
    /// before endpoint and effective-time filtering, including after a repair
    /// moves an endpoint away from the requested asset.
    pub(super) async fn find_asset_snapshot_page(
        &self,
        tenant_id: &TenantId,
        asset_id: &AssetId,
        query: &RelationshipSnapshotPageQuery,
    ) -> Result<Option<RelationshipSnapshotPage>, PostgresRepositoryError> {
        let exists = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM metadata.assets WHERE tenant_id = $1 AND asset_id = $2)",
            tenant_id.as_str(),
            asset_id.as_str()
        )
        .fetch_one(&self.pool)
        .await
        .map_err(classify_database_error)?;
        if !exists.unwrap_or(false) {
            return Ok(None);
        }

        let snapshot = query.snapshot();
        let rows = sqlx::query_as!(
            RelationshipRevisionRow,
            r"
            WITH candidate_relationships AS (
                SELECT DISTINCT relationship_id
                FROM metadata.asset_relationship_revisions
                WHERE tenant_id = $1
                  AND recorded_at_ms <= $4
                  AND (source_asset_id = $2 OR target_asset_id = $2)
                  AND ($5::text IS NULL OR relationship_id > $5)
            ),
            known_relationships AS (
                SELECT revision.*
                FROM candidate_relationships AS candidate
                CROSS JOIN LATERAL (
                    SELECT stored_revision.*
                    FROM metadata.asset_relationship_revisions AS stored_revision
                    WHERE stored_revision.tenant_id = $1
                      AND stored_revision.relationship_id = candidate.relationship_id
                      AND stored_revision.recorded_at_ms <= $4
                    ORDER BY stored_revision.revision DESC
                    LIMIT 1
                ) AS revision
            )
            SELECT
                relationship_id, revision, recorded_at_ms,
                relationship_type_id, relationship_type_version,
                source_asset_id, target_asset_id,
                effective_start_ms, effective_end_ms
            FROM known_relationships
            WHERE (source_asset_id = $2 OR target_asset_id = $2)
              AND effective_start_ms <= $3
              AND (effective_end_ms IS NULL OR $3 < effective_end_ms)
            ORDER BY relationship_id
            LIMIT $6
            ",
            tenant_id.as_str(),
            asset_id.as_str(),
            snapshot.effective_at().get(),
            snapshot.known_at().get(),
            query.after().map(AssetRelationshipId::as_str),
            i64::from(query.limit()) + 1,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(classify_database_error)?;
        let mut relationships = rows
            .into_iter()
            .map(|row| row.into_domain(tenant_id.clone()))
            .collect::<Result<Vec<_>, _>>()?;
        let has_more = relationships.len() > usize::from(query.limit());
        relationships.truncate(usize::from(query.limit()));
        let next_after = if has_more {
            relationships
                .last()
                .map(|relationship| relationship.id().clone())
        } else {
            None
        };
        Ok(Some(RelationshipSnapshotPage::new(
            relationships,
            next_after,
        )))
    }

    /// Reads and validates one exact immutable relationship revision.
    pub(super) async fn find_exact_revision(
        &self,
        tenant_id: &TenantId,
        relationship_id: &AssetRelationshipId,
        revision: RecordRevision,
    ) -> Result<Option<AssetRelationship>, PostgresRepositoryError> {
        let revision = BigDecimal::from(revision.get());
        let row = sqlx::query_as!(
            RelationshipRevisionRow,
            r"
            SELECT
                relationship_id,
                revision,
                recorded_at_ms,
                relationship_type_id,
                relationship_type_version,
                source_asset_id,
                target_asset_id,
                effective_start_ms,
                effective_end_ms
            FROM metadata.asset_relationship_revisions
            WHERE tenant_id = $1
              AND relationship_id = $2
              AND revision = $3
            ",
            tenant_id.as_str(),
            relationship_id.as_str(),
            revision
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(classify_database_error)?;

        row.map(|row| row.into_domain(tenant_id.clone()))
            .transpose()
    }

    /// Reconstructs the relationships involving an asset at a bitemporal point.
    ///
    /// The query first selects candidate identities that involved the asset by
    /// the knowledge cutoff. It then selects each identity's latest known
    /// revision before applying endpoint and half-open effective-time filters.
    /// This ordering is required because a correction can change an endpoint.
    pub(super) async fn find_asset_snapshot(
        &self,
        tenant_id: &TenantId,
        asset_id: &AssetId,
        query: RelationshipSnapshotQuery,
    ) -> Result<Vec<AssetRelationship>, PostgresRepositoryError> {
        let rows = sqlx::query_as!(
            RelationshipRevisionRow,
            r"
            WITH candidate_relationships AS (
                SELECT DISTINCT relationship_id
                FROM metadata.asset_relationship_revisions
                WHERE tenant_id = $1
                  AND recorded_at_ms <= $4
                  AND (source_asset_id = $2 OR target_asset_id = $2)
            ),
            known_relationships AS (
                SELECT revision.*
                FROM candidate_relationships AS candidate
                CROSS JOIN LATERAL (
                    SELECT stored_revision.*
                    FROM metadata.asset_relationship_revisions AS stored_revision
                    WHERE stored_revision.tenant_id = $1
                      AND stored_revision.relationship_id = candidate.relationship_id
                      AND stored_revision.recorded_at_ms <= $4
                    ORDER BY stored_revision.revision DESC
                    LIMIT 1
                ) AS revision
            )
            SELECT
                relationship_id,
                revision,
                recorded_at_ms,
                relationship_type_id,
                relationship_type_version,
                source_asset_id,
                target_asset_id,
                effective_start_ms,
                effective_end_ms
            FROM known_relationships
            WHERE (source_asset_id = $2 OR target_asset_id = $2)
              AND effective_start_ms <= $3
              AND (effective_end_ms IS NULL OR $3 < effective_end_ms)
            ORDER BY relationship_id
            ",
            tenant_id.as_str(),
            asset_id.as_str(),
            query.effective_at().get(),
            query.known_at().get()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(classify_database_error)?;

        rows.into_iter()
            .map(|row| row.into_domain(tenant_id.clone()))
            .collect()
    }
}
