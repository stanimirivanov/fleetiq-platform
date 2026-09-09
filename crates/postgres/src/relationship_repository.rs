use std::future::Future;

use bigdecimal::BigDecimal;
use fleetiq_application::{RelationshipRepository, RelationshipSnapshotQuery};
use fleetiq_domain::{
    AssetId, AssetRelationship, AssetRelationshipId, DefinitionVersion, EffectiveInterval,
    RecordRevision, RecordedRevision, RelationshipEndpoints, RelationshipTypeId,
    RelationshipTypeRef, TenantId, UnixTimeMillis,
};
use sqlx::PgPool;

use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};

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

    /// Appends one complete relationship revision transactionally.
    ///
    /// A new identity is created for revision 1. Existing identities are locked
    /// before validating and advancing their head, which serializes concurrent
    /// writers for that relationship while leaving unrelated relationships
    /// independent. These statements remain separate because the locked head
    /// must be inspected through domain revision and time invariants before the
    /// immutable revision can be inserted.
    async fn append_revision(
        &self,
        relationship: &AssetRelationship,
    ) -> Result<(), PostgresRepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(classify_database_error)?;
        let tenant_id = relationship.tenant_id().as_str();
        let relationship_id = relationship.id().as_str();
        let revision_number = relationship.revision().get();
        let revision = BigDecimal::from(revision_number);

        let identity_insert = sqlx::query!(
            r"
            INSERT INTO metadata.asset_relationships
                (tenant_id, relationship_id, latest_revision)
            VALUES ($1, $2, $3)
            ON CONFLICT (tenant_id, relationship_id) DO NOTHING
            ",
            tenant_id,
            relationship_id,
            revision
        )
        .execute(&mut *transaction)
        .await
        .map_err(classify_database_error)?;
        let is_first_revision = identity_insert.rows_affected() == 1;

        if is_first_revision {
            if revision_number != 1 {
                return Err(PostgresRepositoryError::UnexpectedRevision {
                    expected: 1,
                    actual: revision_number,
                });
            }
        } else {
            let head = sqlx::query_as!(
                RelationshipHeadRow,
                r"
                SELECT identity.latest_revision, revision.recorded_at_ms
                FROM metadata.asset_relationships AS identity
                JOIN metadata.asset_relationship_revisions AS revision
                  ON revision.tenant_id = identity.tenant_id
                 AND revision.relationship_id = identity.relationship_id
                 AND revision.revision = identity.latest_revision
                WHERE identity.tenant_id = $1
                  AND identity.relationship_id = $2
                FOR UPDATE OF identity
                ",
                tenant_id,
                relationship_id
            )
            .fetch_one(&mut *transaction)
            .await
            .map_err(classify_database_error)?;
            let latest =
                parse_revision("asset_relationships.latest_revision", &head.latest_revision)?;
            let expected = latest
                .get()
                .checked_add(1)
                .ok_or(PostgresRepositoryError::RevisionExhausted)?;

            if revision_number != expected {
                return Err(PostgresRepositoryError::UnexpectedRevision {
                    expected,
                    actual: revision_number,
                });
            }
            if relationship.recorded_at().get() < head.recorded_at_ms {
                return Err(PostgresRepositoryError::RecordedTimeRegression {
                    previous_ms: head.recorded_at_ms,
                    actual_ms: relationship.recorded_at().get(),
                });
            }
        }

        insert_revision_row(&mut transaction, relationship, &revision).await?;

        if !is_first_revision {
            sqlx::query!(
                r"
                UPDATE metadata.asset_relationships
                SET latest_revision = $3
                WHERE tenant_id = $1 AND relationship_id = $2
                ",
                tenant_id,
                relationship_id,
                revision
            )
            .execute(&mut *transaction)
            .await
            .map_err(classify_database_error)?;
        }

        transaction.commit().await.map_err(classify_database_error)
    }

    /// Reads and validates one exact immutable relationship revision.
    async fn find_exact_revision(
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
    async fn find_asset_snapshot(
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

/// Inserts the immutable revision row inside the caller's locked transaction.
///
/// The caller owns sequence and recorded-time validation and advances the
/// stable relationship head before committing the transaction.
async fn insert_revision_row(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    relationship: &AssetRelationship,
    revision: &BigDecimal,
) -> Result<(), PostgresRepositoryError> {
    let interval = relationship.effective_interval();
    let relationship_type = relationship.relationship_type();

    sqlx::query!(
        r"
        INSERT INTO metadata.asset_relationship_revisions
            (tenant_id, relationship_id, revision, recorded_at_ms,
             relationship_type_id, relationship_type_version,
             source_asset_id, target_asset_id,
             effective_start_ms, effective_end_ms)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        ",
        relationship.tenant_id().as_str(),
        relationship.id().as_str(),
        revision,
        relationship.recorded_at().get(),
        relationship_type.id().as_str(),
        i64::from(relationship_type.version().get()),
        relationship.source_asset_id().as_str(),
        relationship.target_asset_id().as_str(),
        interval.start().get(),
        interval.end().map(UnixTimeMillis::get)
    )
    .execute(&mut **transaction)
    .await
    .map_err(classify_database_error)?;

    Ok(())
}

#[derive(Debug)]
struct RelationshipHeadRow {
    latest_revision: BigDecimal,
    recorded_at_ms: i64,
}

#[derive(Debug)]
struct RelationshipRevisionRow {
    relationship_id: String,
    revision: BigDecimal,
    recorded_at_ms: i64,
    relationship_type_id: String,
    relationship_type_version: i64,
    source_asset_id: String,
    target_asset_id: String,
    effective_start_ms: i64,
    effective_end_ms: Option<i64>,
}

impl RelationshipRevisionRow {
    fn into_domain(
        self,
        tenant_id: TenantId,
    ) -> Result<AssetRelationship, PostgresRepositoryError> {
        let interval = EffectiveInterval::new(
            UnixTimeMillis::new(self.effective_start_ms),
            self.effective_end_ms.map(UnixTimeMillis::new),
        )
        .map_err(|error| corrupt("asset_relationship_revisions.effective_interval", error))?;

        Ok(AssetRelationship::new(
            tenant_id,
            AssetRelationshipId::new(self.relationship_id)
                .map_err(|error| corrupt("asset_relationship_revisions.relationship_id", error))?,
            RecordedRevision::new(
                parse_revision("asset_relationship_revisions.revision", &self.revision)?,
                UnixTimeMillis::new(self.recorded_at_ms),
            ),
            RelationshipTypeRef::new(
                RelationshipTypeId::new(self.relationship_type_id).map_err(|error| {
                    corrupt("asset_relationship_revisions.relationship_type_id", error)
                })?,
                definition_version(
                    "asset_relationship_revisions.relationship_type_version",
                    self.relationship_type_version,
                )?,
            ),
            RelationshipEndpoints::new(
                AssetId::new(self.source_asset_id).map_err(|error| {
                    corrupt("asset_relationship_revisions.source_asset_id", error)
                })?,
                AssetId::new(self.target_asset_id).map_err(|error| {
                    corrupt("asset_relationship_revisions.target_asset_id", error)
                })?,
            ),
            interval,
        ))
    }
}

fn parse_revision(
    field: &'static str,
    value: &BigDecimal,
) -> Result<RecordRevision, PostgresRepositoryError> {
    let value = value
        .to_string()
        .parse::<u64>()
        .map_err(|error| corrupt(field, error))?;
    RecordRevision::new(value).map_err(|error| corrupt(field, error))
}

fn definition_version(
    field: &'static str,
    value: i64,
) -> Result<DefinitionVersion, PostgresRepositoryError> {
    let value = u32::try_from(value).map_err(|error| corrupt(field, error))?;
    DefinitionVersion::new(value).map_err(|error| corrupt(field, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_the_complete_record_revision_range() {
        let stored = BigDecimal::from(u64::MAX);

        assert_eq!(
            parse_revision("test.revision", &stored)
                .expect("maximum u64 should remain representable")
                .get(),
            u64::MAX
        );
    }
}
