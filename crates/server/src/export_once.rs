//! Trusted explicit export calls; no listener, scheduler or automatic activation.
use fleetiq_domain::TenantId;
use fleetiq_history::exports::{ExportFailure, ExportKey, ExportRequest, ExportTick, ExportTicket};
use fleetiq_postgres::PostgresExports;
/// Freezes one explicit bounded request; the caller establishes tenant authority.
/// # Errors
/// Returns redacted scope, coverage, quota, codec, persistence and deadline failures.
pub async fn request_export(
    pool: sqlx::PgPool,
    key: &ExportKey,
    request: &ExportRequest,
) -> Result<ExportTicket, ExportFailure> {
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        PostgresExports::new(pool).request(key, request, &fleetiq_archive::ExactExportCodec),
    )
    .await
    .map_err(|_| ExportFailure::Storage)?
}
/// Produces one artifact from the frozen generation, never by querying live history.
/// Cancellation retains uncertain running capacity until the thirty-second lease expires.
/// # Errors
/// Returns redacted codec/persistence/deadline failures; durable work remains retryable.
pub async fn export_once(
    pool: sqlx::PgPool,
    tenant: &TenantId,
    ticket: ExportTicket,
) -> Result<ExportTick, ExportFailure> {
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        fleetiq_history::exports::export_once(
            &PostgresExports::new(pool),
            &fleetiq_archive::ExactExportCodec,
            tenant,
            ticket,
        ),
    )
    .await
    .map_err(|_| ExportFailure::Storage)?
}
