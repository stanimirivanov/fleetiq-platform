//! Bounded job metadata for protected status and delivery preparation.
use super::{ExportFailure, ExportStatus, ExportTicket, PostgresExports};
use fleetiq_domain::TenantId;
use fleetiq_history::{exports::ExportInfo, restore::RestoreTicket};
impl PostgresExports {
    /// Describes one active tenant's job without reading its evidence file.
    /// # Errors
    /// Returns redacted storage or persisted-contract failures.
    pub async fn describe(
        &self,
        tenant: &TenantId,
        ticket: ExportTicket,
    ) -> Result<Option<ExportInfo>, ExportFailure> {
        let mut tx = self
            .transaction(tenant)
            .await
            .map_err(|_| ExportFailure::Storage)?;
        let row=sqlx::query!("SELECT CASE WHEN expires_at<=clock_timestamp() THEN 'expired' ELSE status END AS \"state!\",floor(extract(epoch FROM expires_at)*1000)::bigint AS \"expires!\",row_count,file_bytes,manifest_id,restore_generation FROM history.export_jobs j JOIN metadata.tenants t USING(tenant_id) WHERE j.tenant_id=$1 AND job_id=$2 AND t.is_active",tenant.as_str(),ticket.id()).fetch_optional(&mut *tx).await.map_err(|_|ExportFailure::Storage)?;
        tx.commit().await.map_err(|_| ExportFailure::Storage)?;
        row.map(|r| {
            let status = match r.state.as_str() {
                "pending" => ExportStatus::Pending,
                "ready" => ExportStatus::Ready,
                "cancelled" => ExportStatus::Cancelled,
                "expired" => ExportStatus::Expired,
                _ => return Err(ExportFailure::Storage),
            };
            let restore = match (r.manifest_id, r.restore_generation) {
                (None, None) => None,
                (Some(m), Some(g)) => {
                    Some(RestoreTicket::new(m, g).map_err(|_| ExportFailure::Storage)?)
                }
                _ => return Err(ExportFailure::Storage),
            };
            Ok(ExportInfo {
                ticket,
                status,
                expires_at_ms: r.expires,
                rows: u8::try_from(r.row_count).map_err(|_| ExportFailure::Storage)?,
                file_bytes: r.file_bytes,
                restore,
            })
        })
        .transpose()
    }
}
