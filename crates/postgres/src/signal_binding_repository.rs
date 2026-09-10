use std::future::Future;

use bigdecimal::BigDecimal;
use fleetiq_application::{SignalBindingRepository, SignalBindingSnapshotQuery};
use fleetiq_domain::{
    AssetId, DefinitionVersion, DeviceEndpointId, EffectiveInterval, PropertyDefinitionId,
    PropertyDefinitionRef, RecordRevision, RecordedRevision, SignalBinding, SignalBindingId,
    SignalSource, SignalTarget, SourceSignalId, TenantId, UnixTimeMillis,
};
use sqlx::PgPool;

use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};

/// PostgreSQL implementation of revisioned signal attribution persistence.
#[derive(Clone, Debug)]
pub struct PostgresSignalBindingRepository {
    pool: PgPool,
}

impl PostgresSignalBindingRepository {
    /// Constructs a signal-binding repository from an existing connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Appends one complete binding revision transactionally.
    ///
    /// Later writers lock the stable identity's head before checking sequence
    /// and recorded-time invariants. This serializes concurrent changes to one
    /// binding while unrelated bindings can proceed independently.
    async fn append_revision(
        &self,
        binding: &SignalBinding,
    ) -> Result<(), PostgresRepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(classify_database_error)?;
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
                SignalBindingHeadRow,
                r"
                SELECT identity.latest_revision, revision.recorded_at_ms
                FROM metadata.signal_bindings AS identity
                JOIN metadata.signal_binding_revisions AS revision
                  ON revision.tenant_id = identity.tenant_id
                 AND revision.binding_id = identity.binding_id
                 AND revision.revision = identity.latest_revision
                WHERE identity.tenant_id = $1
                  AND identity.binding_id = $2
                FOR UPDATE OF identity
                ",
                tenant_id,
                binding_id
            )
            .fetch_one(&mut *transaction)
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
            if binding.recorded_at().get() < head.recorded_at_ms {
                return Err(PostgresRepositoryError::RecordedTimeRegression {
                    previous_ms: head.recorded_at_ms,
                    actual_ms: binding.recorded_at().get(),
                });
            }
        }

        insert_revision_row(&mut transaction, binding, &revision).await?;

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
            .execute(&mut *transaction)
            .await
            .map_err(classify_database_error)?;
        }

        transaction.commit().await.map_err(classify_database_error)
    }

    /// Reads and validates one exact immutable binding revision.
    async fn find_exact_revision(
        &self,
        tenant_id: &TenantId,
        binding_id: &SignalBindingId,
        revision: RecordRevision,
    ) -> Result<Option<SignalBinding>, PostgresRepositoryError> {
        let revision = BigDecimal::from(revision.get());
        let row = sqlx::query_as!(
            SignalBindingRevisionRow,
            r"
            SELECT binding_id, revision, recorded_at_ms,
                   source_device_asset_id, source_endpoint_id, source_signal_id,
                   target_asset_id, target_property_id, target_property_version,
                   effective_start_ms, effective_end_ms
            FROM metadata.signal_binding_revisions
            WHERE tenant_id = $1 AND binding_id = $2 AND revision = $3
            ",
            tenant_id.as_str(),
            binding_id.as_str(),
            revision
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(classify_database_error)?;

        row.map(|row| row.into_domain(tenant_id.clone()))
            .transpose()
    }

    /// Resolves attributions from one decoded source at a bitemporal point.
    ///
    /// Candidate discovery precedes latest-known revision selection. The final
    /// source filter removes a binding when a later correction moved it away
    /// from the requested source.
    async fn find_source_snapshot(
        &self,
        tenant_id: &TenantId,
        source: &SignalSource,
        query: SignalBindingSnapshotQuery,
    ) -> Result<Vec<SignalBinding>, PostgresRepositoryError> {
        let rows = sqlx::query_as!(
            SignalBindingRevisionRow,
            r"
            WITH candidate_bindings AS (
                SELECT DISTINCT binding_id
                FROM metadata.signal_binding_revisions
                WHERE tenant_id = $1
                  AND source_device_asset_id = $2
                  AND source_endpoint_id = $3
                  AND source_signal_id = $4
                  AND recorded_at_ms <= $6
            ),
            known_bindings AS (
                SELECT revision.*
                FROM candidate_bindings AS candidate
                CROSS JOIN LATERAL (
                    SELECT stored_revision.*
                    FROM metadata.signal_binding_revisions AS stored_revision
                    WHERE stored_revision.tenant_id = $1
                      AND stored_revision.binding_id = candidate.binding_id
                      AND stored_revision.recorded_at_ms <= $6
                    ORDER BY stored_revision.revision DESC
                    LIMIT 1
                ) AS revision
            )
            SELECT binding_id, revision, recorded_at_ms,
                   source_device_asset_id, source_endpoint_id, source_signal_id,
                   target_asset_id, target_property_id, target_property_version,
                   effective_start_ms, effective_end_ms
            FROM known_bindings
            WHERE source_device_asset_id = $2
              AND source_endpoint_id = $3
              AND source_signal_id = $4
              AND effective_start_ms <= $5
              AND (effective_end_ms IS NULL OR $5 < effective_end_ms)
            ORDER BY binding_id
            ",
            tenant_id.as_str(),
            source.device_asset_id().as_str(),
            source.endpoint_id().as_str(),
            source.signal_id().as_str(),
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

impl SignalBindingRepository for PostgresSignalBindingRepository {
    type Error = PostgresRepositoryError;

    fn insert_revision(
        &self,
        binding: &SignalBinding,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.append_revision(binding)
    }

    fn find_revision(
        &self,
        tenant_id: &TenantId,
        binding_id: &SignalBindingId,
        revision: RecordRevision,
    ) -> impl Future<Output = Result<Option<SignalBinding>, Self::Error>> + Send {
        self.find_exact_revision(tenant_id, binding_id, revision)
    }

    fn find_for_source(
        &self,
        tenant_id: &TenantId,
        source: &SignalSource,
        query: SignalBindingSnapshotQuery,
    ) -> impl Future<Output = Result<Vec<SignalBinding>, Self::Error>> + Send {
        self.find_source_snapshot(tenant_id, source, query)
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
    recorded_at_ms: i64,
}

#[derive(Debug)]
struct SignalBindingRevisionRow {
    binding_id: String,
    revision: BigDecimal,
    recorded_at_ms: i64,
    source_device_asset_id: String,
    source_endpoint_id: String,
    source_signal_id: String,
    target_asset_id: String,
    target_property_id: String,
    target_property_version: i64,
    effective_start_ms: i64,
    effective_end_ms: Option<i64>,
}

impl SignalBindingRevisionRow {
    fn into_domain(self, tenant_id: TenantId) -> Result<SignalBinding, PostgresRepositoryError> {
        let interval = EffectiveInterval::new(
            UnixTimeMillis::new(self.effective_start_ms),
            self.effective_end_ms.map(UnixTimeMillis::new),
        )
        .map_err(|error| corrupt("signal_binding_revisions.effective_interval", error))?;

        Ok(SignalBinding::new(
            tenant_id,
            SignalBindingId::new(self.binding_id)
                .map_err(|error| corrupt("signal_binding_revisions.binding_id", error))?,
            RecordedRevision::new(
                parse_revision("signal_binding_revisions.revision", &self.revision)?,
                UnixTimeMillis::new(self.recorded_at_ms),
            ),
            SignalSource::new(
                AssetId::new(self.source_device_asset_id).map_err(|error| {
                    corrupt("signal_binding_revisions.source_device_asset_id", error)
                })?,
                DeviceEndpointId::new(self.source_endpoint_id).map_err(|error| {
                    corrupt("signal_binding_revisions.source_endpoint_id", error)
                })?,
                SourceSignalId::new(self.source_signal_id)
                    .map_err(|error| corrupt("signal_binding_revisions.source_signal_id", error))?,
            ),
            SignalTarget::new(
                AssetId::new(self.target_asset_id)
                    .map_err(|error| corrupt("signal_binding_revisions.target_asset_id", error))?,
                PropertyDefinitionRef::new(
                    PropertyDefinitionId::new(self.target_property_id).map_err(|error| {
                        corrupt("signal_binding_revisions.target_property_id", error)
                    })?,
                    definition_version(
                        "signal_binding_revisions.target_property_version",
                        self.target_property_version,
                    )?,
                ),
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
