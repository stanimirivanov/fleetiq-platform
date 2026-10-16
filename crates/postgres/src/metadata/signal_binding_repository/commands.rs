use bigdecimal::BigDecimal;
use fleetiq_domain::{SignalBinding, UnixTimeMillis};

use crate::error::{PostgresRepositoryError, classify_database_error};

use super::{PostgresSignalBindingRepository, common::parse_revision};

impl PostgresSignalBindingRepository {
    /// Appends one complete binding revision transactionally.
    ///
    /// Later writers lock the stable identity's head before checking sequence
    /// and recorded-time invariants. This serializes concurrent changes to one
    /// binding while unrelated bindings can proceed independently.
    pub(super) async fn append_revision(
        &self,
        binding: &SignalBinding,
    ) -> Result<(), PostgresRepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(classify_database_error)?;
        Self::append_revision_on(&mut transaction, binding).await?;
        transaction.commit().await.map_err(classify_database_error)
    }

    /// Appends under the caller-owned transaction so attribution commits atomically.
    ///
    /// Preserve head lock ordering, fresh dependent snapshots and strict errors.
    pub(in crate::metadata) async fn append_revision_on(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        binding: &SignalBinding,
    ) -> Result<(), PostgresRepositoryError> {
        let tenant_id = binding.tenant_id().as_str();
        let binding_id = binding.id().as_str();
        let revision_number = binding.revision().get();
        let revision = BigDecimal::from(revision_number);

        let identity_insert = sqlx::query!(
            r"
            INSERT INTO metadata.signal_bindings
                (tenant_id, binding_id, latest_revision)
            VALUES ($1, $2, $3)
            ON CONFLICT (tenant_id, binding_id) DO NOTHING
            ",
            tenant_id,
            binding_id,
            revision
        )
        .execute(&mut **transaction)
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
            // Lock only the mutable identity row. A joined revision row would
            // use the snapshot taken before a concurrent lock wait and could
            // disappear during PostgreSQL's row recheck after the winner
            // advances `latest_revision`.
            let head = sqlx::query_as!(
                SignalBindingHeadRow,
                r"
                SELECT identity.latest_revision
                FROM metadata.signal_bindings AS identity
                WHERE identity.tenant_id = $1
                  AND identity.binding_id = $2
                FOR UPDATE
                ",
                tenant_id,
                binding_id
            )
            .fetch_one(&mut **transaction)
            .await
            .map_err(classify_database_error)?;
            let latest = parse_revision("signal_bindings.latest_revision", &head.latest_revision)?;
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

            // This separate statement starts after the identity lock is held,
            // so READ COMMITTED supplies a fresh snapshot containing the
            // winning concurrent revision identified by the locked head.
            let recorded_at_ms = sqlx::query_scalar!(
                r"
                SELECT recorded_at_ms
                FROM metadata.signal_binding_revisions
                WHERE tenant_id = $1
                  AND binding_id = $2
                  AND revision = $3
                ",
                tenant_id,
                binding_id,
                head.latest_revision
            )
            .fetch_one(&mut **transaction)
            .await
            .map_err(classify_database_error)?;

            if binding.recorded_at().get() < recorded_at_ms {
                return Err(PostgresRepositoryError::RecordedTimeRegression {
                    previous_ms: recorded_at_ms,
                    actual_ms: binding.recorded_at().get(),
                });
            }
        }

        insert_revision_row(transaction, binding, &revision).await?;

        if !is_first_revision {
            sqlx::query!(
                r"
                UPDATE metadata.signal_bindings
                SET latest_revision = $3
                WHERE tenant_id = $1 AND binding_id = $2
                ",
                tenant_id,
                binding_id,
                revision
            )
            .execute(&mut **transaction)
            .await
            .map_err(classify_database_error)?;
        }

        Ok(())
    }
}

/// Inserts the immutable row after the caller validates the locked head.
async fn insert_revision_row(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    binding: &SignalBinding,
    revision: &BigDecimal,
) -> Result<(), PostgresRepositoryError> {
    let source = binding.source();
    let target = binding.target();
    let property = target.property();
    let interval = binding.effective_interval();

    sqlx::query!(
        r"
        INSERT INTO metadata.signal_binding_revisions
            (tenant_id, binding_id, revision, recorded_at_ms,
             source_device_asset_id, source_endpoint_id, source_signal_id,
             target_asset_id, target_property_id, target_property_version,
             effective_start_ms, effective_end_ms)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        ",
        binding.tenant_id().as_str(),
        binding.id().as_str(),
        revision,
        binding.recorded_at().get(),
        source.device_asset_id().as_str(),
        source.endpoint_id().as_str(),
        source.signal_id().as_str(),
        target.asset_id().as_str(),
        property.id().as_str(),
        i64::from(property.version().get()),
        interval.start().get(),
        interval.end().map(UnixTimeMillis::get)
    )
    .execute(&mut **transaction)
    .await
    .map_err(classify_database_error)?;

    Ok(())
}

#[derive(Debug)]
struct SignalBindingHeadRow {
    latest_revision: BigDecimal,
}
