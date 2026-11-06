//! Snapshot reconciliation of authoritative rows, without mutable cached counters.
use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};
use fleetiq_access::{AuthorizedTenant, ReadOperations};
use fleetiq_history::storage_usage::{StorageMeasure, StorageUsage};
use sqlx::PgPool;
/// Tenant-scoped live logical accounting with a database deadline.
pub struct PostgresStorageUsage(PgPool);
impl PostgresStorageUsage {
    /// Uses the shared migrated application pool.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    /// Reconciles three storage classes in one repeatable-read snapshot.
    /// # Errors
    /// Fails closed on timeout, database error, or invalid/overflowing totals.
    pub async fn snapshot(
        &self,
        auth: AuthorizedTenant<ReadOperations>,
    ) -> Result<StorageUsage, PostgresRepositoryError> {
        let mut tx = self.0.begin().await.map_err(classify_database_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(classify_database_error)?;
        sqlx::query("SET LOCAL statement_timeout = '2s'")
            .execute(&mut *tx)
            .await
            .map_err(classify_database_error)?;
        let row = sqlx::query!(r#"
SELECT floor(extract(epoch FROM transaction_timestamp())*1000)::bigint AS "observed!",
 (SELECT count(*)::text FROM ingestion.accepted_messages WHERE tenant_id=$1) AS "accepted_records!",
 (SELECT COALESCE(sum(octet_length(payload)::numeric),0)::text FROM ingestion.accepted_messages WHERE tenant_id=$1) AS "accepted_bytes!",
 (SELECT count(*)::text FROM ingestion.decoded_observations WHERE tenant_id=$1) AS "decoded_records!",
 (SELECT COALESCE(sum(octet_length(convert_to(row_to_json(d)::text,'UTF8'))::numeric),0)::text FROM ingestion.decoded_observations d WHERE tenant_id=$1) AS "decoded_bytes!",
 (SELECT count(*)::text FROM history.asset_property_changes WHERE tenant_id=$1) AS "interpretation_records!",
 (SELECT COALESCE(sum(octet_length(convert_to(row_to_json(h)::text,'UTF8'))::numeric),0)::text FROM history.asset_property_changes h WHERE tenant_id=$1) AS "interpretation_bytes!"
"#, auth.tenant_id().as_str()).fetch_one(&mut *tx).await.map_err(classify_database_error)?;
        let measure = |n: &str, b: &str| {
            StorageMeasure::from_totals(n, b).map_err(|e| corrupt("storage_usage", e))
        };
        let result = StorageUsage {
            observed_at_ms: row.observed,
            accepted: measure(&row.accepted_records, &row.accepted_bytes)?,
            decoded: measure(&row.decoded_records, &row.decoded_bytes)?,
            interpretations: measure(&row.interpretation_records, &row.interpretation_bytes)?,
        };
        tx.commit().await.map_err(classify_database_error)?;
        Ok(result)
    }
}
