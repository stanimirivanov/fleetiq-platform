//! Explicit bounded restore attempt for trusted callers and the protected HTTP command.
use fleetiq_archive::objects::{ImmutableArchiveObjects, S3ArchiveConfig};
use fleetiq_domain::TenantId;
use fleetiq_history::restore::{RestoreFailure, RestoreTick, RestoreTicket};
use fleetiq_postgres::PostgresRestore;
/// Restores one explicit request using operator inputs and an already migrated pool.
/// The caller establishes tenant authority. The twenty-second turn deadline is
/// shorter than the thirty-second reservation/lease and remaining request lifetime.
/// # Errors
/// Returns redacted configuration, verification, database or deadline failures.
pub async fn restore_once(
    pool: sqlx::PgPool,
    config: S3ArchiveConfig,
    tenant: &TenantId,
    ticket: &RestoreTicket,
) -> Result<RestoreTick, RestoreFailure> {
    let source = ImmutableArchiveObjects::connect(config).map_err(|_| RestoreFailure)?;
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        fleetiq_history::restore::restore_once(
            &PostgresRestore::new(pool),
            &source,
            tenant,
            *ticket,
        ),
    )
    .await
    .map_err(|_| RestoreFailure)?
}
