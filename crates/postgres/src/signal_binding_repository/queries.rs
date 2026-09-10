use bigdecimal::BigDecimal;
use fleetiq_application::SignalBindingSnapshotQuery;
use fleetiq_domain::{RecordRevision, SignalBinding, SignalBindingId, SignalSource, TenantId};

use crate::error::{PostgresRepositoryError, classify_database_error};

use super::{PostgresSignalBindingRepository, row::SignalBindingRevisionRow};

impl PostgresSignalBindingRepository {
    /// Reads and validates one exact immutable binding revision.
    pub(super) async fn find_exact_revision(
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
    pub(super) async fn find_source_snapshot(
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
