//! Single-statement request/expiry/file snapshot for explicit restored-history queries.
use super::PostgresRestore;
use fleetiq_domain::TenantId;
use fleetiq_history::{
    RestoredHistorySnapshot,
    restore::{RestoreFailure, RestoreTicket},
};
impl PostgresRestore {
    /// Reads only one selected manifest/request and its verified unexpired file.
    /// # Errors
    /// Returns redacted database failures; unavailable coverage remains explicit.
    pub async fn history_snapshot(
        &self,
        tenant: &TenantId,
        ticket: RestoreTicket,
    ) -> Result<RestoredHistorySnapshot, RestoreFailure> {
        let mut tx = self.transaction().await.map_err(|_| RestoreFailure)?;
        let row = sqlx::query_file!(
            "src/history/restore/history_snapshot.sql",
            tenant.as_str(),
            ticket.manifest(),
            ticket.generation()
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| RestoreFailure)?;
        tx.commit().await.map_err(|_| RestoreFailure)?;
        Ok(match row {
            Some(row) => match (row.state.as_str(), row.payload) {
                ("archived", _) => RestoredHistorySnapshot::Archived,
                ("restoring", _) => RestoredHistorySnapshot::Restoring,
                ("restored", Some(bytes)) => RestoredHistorySnapshot::Ready(bytes),
                _ => RestoredHistorySnapshot::Unavailable,
            },
            None => RestoredHistorySnapshot::Unavailable,
        })
    }
}
