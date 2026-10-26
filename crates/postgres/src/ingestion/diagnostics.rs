//! Read-only, repeatable bounded diagnostic projections; never selects opaque bytes.
use crate::error::{PostgresRepositoryError, classify_database_error};
use fleetiq_domain::{AssetId, TenantId};
use fleetiq_ingestion::{
    DIAGNOSTIC_FRAME_LIMIT, DIAGNOSTIC_OUTPUT_LIMIT, DIAGNOSTIC_SOURCE_LIMIT, DiagnosticFrame,
    IngestionDiagnostics, IngestionDiagnosticsRepository, ObservedSource, ProcessingEvidence,
};
use sqlx::PgPool;

/// Operational evidence adapter; independent of current device/socket liveness.
#[derive(Clone, Debug)]
pub struct PostgresIngestionDiagnostics {
    pool: PgPool,
}
impl PostgresIngestionDiagnostics {
    /// Shares the application's bounded pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
impl IngestionDiagnosticsRepository for PostgresIngestionDiagnostics {
    type Error = PostgresRepositoryError;
    async fn snapshot(
        &self,
        tenant: &TenantId,
        device: &AssetId,
    ) -> Result<Option<IngestionDiagnostics>, Self::Error> {
        // One snapshot keeps receipt selection, completion and source coverage
        // consistent while ingestion commits concurrently. Fixed-size queries
        // avoid N+1 lookups and never read payload, state or measurement values.
        let mut tx = self.pool.begin().await.map_err(classify_database_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(classify_database_error)?;
        sqlx::query("SET LOCAL statement_timeout = '2s'")
            .execute(&mut *tx)
            .await
            .map_err(classify_database_error)?;
        let scope = sqlx::query!(
            r#"
            SELECT EXISTS(SELECT 1 FROM metadata.communication_devices
                WHERE tenant_id=$1 AND device_asset_id=$2) AS "present!",
                floor(extract(epoch FROM transaction_timestamp())*1000)::bigint AS "now!"
            "#,
            tenant.as_str(),
            device.as_str()
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        if !scope.present {
            tx.commit().await.map_err(classify_database_error)?;
            return Ok(None);
        }
        // LIMIT precedes exact-key joins. The extra receipt is only a coverage
        // sentinel and is never used for source discovery or returned to callers.
        let mut rows = sqlx::query!(r#"
            WITH recent AS MATERIALIZED (
                SELECT tenant_id,message_id,protocol_package_id,protocol_package_version,received_at_ms
                FROM ingestion.accepted_messages WHERE tenant_id=$1 AND device_asset_id=$2
                ORDER BY received_at_ms DESC,message_id DESC LIMIT $3
            )
            SELECT m.message_id,m.protocol_package_id,m.protocol_package_version,m.received_at_ms,
                   d.observation_count,w.status,w.lease_until_ms
            FROM recent m LEFT JOIN ingestion.decoded_message_results d
                USING(tenant_id,message_id,protocol_package_id,protocol_package_version)
            LEFT JOIN ingestion.replay_work w USING(tenant_id,message_id)
            ORDER BY m.received_at_ms DESC,m.message_id DESC
            "#, tenant.as_str(),device.as_str(), i64::try_from(DIAGNOSTIC_FRAME_LIMIT+1).unwrap_or(65))
            .fetch_all(&mut *tx).await.map_err(classify_database_error)?;
        let frames_truncated = rows.len() > DIAGNOSTIC_FRAME_LIMIT;
        rows.truncate(DIAGNOSTIC_FRAME_LIMIT);
        let observations_truncated = rows
            .iter()
            .any(|r| r.observation_count.is_some_and(|n| n > 64));
        // At most 64 receipts x 64 positions enter DISTINCT. The per-message
        // primary key supports each lateral seek; the final sentinel detects
        // distinct-source truncation without materializing arbitrary history.
        let mut sources = sqlx::query!(r#"
            WITH recent AS MATERIALIZED (
                SELECT tenant_id,message_id,protocol_package_id,protocol_package_version
                FROM ingestion.accepted_messages WHERE tenant_id=$1 AND device_asset_id=$2
                ORDER BY received_at_ms DESC,message_id DESC LIMIT $3
            )
            SELECT DISTINCT m.protocol_package_id,m.protocol_package_version,o.endpoint_id,o.signal_id
            FROM recent m CROSS JOIN LATERAL (
                SELECT endpoint_id,signal_id FROM ingestion.decoded_observations o
                WHERE o.tenant_id=m.tenant_id AND o.message_id=m.message_id
                  AND o.protocol_package_id=m.protocol_package_id
                  AND o.protocol_package_version=m.protocol_package_version
                ORDER BY output_index LIMIT $4
            ) o
            ORDER BY m.protocol_package_id,m.protocol_package_version,o.endpoint_id,o.signal_id LIMIT $5
            "#,tenant.as_str(),device.as_str(), i64::try_from(DIAGNOSTIC_FRAME_LIMIT).unwrap_or(64),
            i64::try_from(DIAGNOSTIC_OUTPUT_LIMIT).unwrap_or(64),i64::try_from(DIAGNOSTIC_SOURCE_LIMIT+1).unwrap_or(129))
            .fetch_all(&mut *tx).await.map_err(classify_database_error)?;
        let sources_truncated = sources.len() > DIAGNOSTIC_SOURCE_LIMIT;
        sources.truncate(DIAGNOSTIC_SOURCE_LIMIT);
        tx.commit().await.map_err(classify_database_error)?;
        Ok(Some(IngestionDiagnostics {
            observed_at_ms: scope.now,
            frames_truncated,
            observations_truncated,
            sources_truncated,
            frames: rows
                .into_iter()
                .map(|r| DiagnosticFrame {
                    processing: ProcessingEvidence::classify(
                        r.observation_count.is_some(),
                        r.status.as_deref(),
                        r.lease_until_ms,
                        scope.now,
                    ),
                    message_id: r.message_id,
                    package_id: r.protocol_package_id,
                    package_version: r.protocol_package_version,
                    received_at_ms: r.received_at_ms,
                    observation_count: r.observation_count,
                })
                .collect(),
            sources: sources
                .into_iter()
                .map(|r| ObservedSource {
                    package_id: r.protocol_package_id,
                    package_version: r.protocol_package_version,
                    endpoint_id: r.endpoint_id,
                    signal_id: r.signal_id,
                })
                .collect(),
        }))
    }
}
