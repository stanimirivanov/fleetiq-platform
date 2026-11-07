//! Explicit single-tenant archive processing with bounded turns and durable retry.
use crate::{SchemaMode, archive_once::archive_turn, config::archive_from_env};
use fleetiq_archive::objects::ImmutableArchiveObjects;
use fleetiq_domain::TenantId;
use fleetiq_postgres::PostgresArchiveWork;
use std::{error::Error, time::Duration};

/// Runs verified archive publication for the configured tenant until shutdown.
/// Requires `FLEETIQ_ARCHIVE_TENANT_ID`, the complete archive storage inputs and
/// a database whose migration inventory already matches this binary.
/// # Errors
/// Returns redacted startup or shutdown errors. Interrupted work retains its lease.
pub async fn run_archive_worker() -> Result<(), Box<dyn Error>> {
    let tenant =
        std::env::var("FLEETIQ_ARCHIVE_TENANT_ID").map_err(|_| "archive worker tenant required")?;
    let tenant = TenantId::new(tenant).map_err(|_| "archive worker tenant invalid")?;
    let archive = archive_from_env()?.ok_or("archive worker storage configuration required")?;
    let store = ImmutableArchiveObjects::connect(archive)
        .map_err(|_| "archive worker storage configuration invalid")?;
    let url = std::env::var("DATABASE_URL").map_err(|_| "database configuration required")?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&url)
        .await
        .map_err(|_| "archive worker database unavailable")?;
    let result = async {
        tokio::time::timeout(Duration::from_secs(5), SchemaMode::Check.prepare(&pool))
            .await
            .map_err(|_| "archive worker schema check timed out")?
            .map_err(|_| "archive worker schema incompatible")?;
        let repository = PostgresArchiveWork::new(pool.clone());
        let shutdown = crate::runtime::shutdown_signal();
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                biased;
                () = &mut shutdown => break,
                result = archive_turn(&repository, &store, &tenant) => {
                    if result.is_err() {
                        eprintln!("archive worker turn failed; durable retry retained");
                    }
                }
            }
            tokio::select! {
                biased;
                () = &mut shutdown => break,
                () = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
        }
        Ok::<(), Box<dyn Error>>(())
    }
    .await;
    tokio::time::timeout(Duration::from_secs(5), pool.close())
        .await
        .map_err(|_| "archive worker shutdown deadline exceeded")?;
    result
}
