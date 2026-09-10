use bigdecimal::BigDecimal;
use fleetiq_domain::{AssetRelationship, UnixTimeMillis};

use crate::error::{PostgresRepositoryError, classify_database_error};

use super::{PostgresRelationshipRepository, common::parse_revision};

impl PostgresRelationshipRepository {
    /// Appends one complete relationship revision transactionally.
    ///
    /// A new identity is created for revision 1. Existing identities are locked
    /// before validating and advancing their head, which serializes concurrent
    /// writers for that relationship while leaving unrelated relationships
    /// independent. These statements remain separate because the locked head
    /// must be inspected through domain revision and time invariants before the
    /// immutable revision can be inserted.
    pub(super) async fn append_revision(
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
