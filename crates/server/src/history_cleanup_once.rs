//! Trusted expiry reconciliation for one explicit tenant.
use crate::SchemaMode;
use fleetiq_domain::TenantId;
use fleetiq_postgres::{PostgresExports, PostgresRestore};
use std::{env, error::Error, time::Duration};

/// Committed row counts from one successful cleanup turn.
pub struct HistoryCleanupCounts {
    /// Restore requests expired.
    pub restores: u64,
    /// Export jobs expired.
    pub exports: u64,
    /// Stale export attempt leases cleared.
    pub export_leases: u64,
}

/// Checks the installed schema and reconciles one bounded expiry batch per class.
/// The two repositories commit independently. Retrying the same tenant is safe.
/// # Errors
/// Returns redacted configuration, schema, deadline or cleanup failures.
pub async fn run_expire_history_once() -> Result<HistoryCleanupCounts, Box<dyn Error>> {
    let tenant = env::var("FLEETIQ_HISTORY_CLEANUP_TENANT_ID")
        .ok()
        .and_then(|value| TenantId::new(value).ok())
        .ok_or("invalid history cleanup tenant")?;
    let url = env::var("DATABASE_URL").map_err(|_| "database configuration required")?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&url)
        .await
        .map_err(|_| "history cleanup database unavailable")?;
    tokio::time::timeout(Duration::from_secs(5), SchemaMode::Check.prepare(&pool))
        .await
        .map_err(|_| "history cleanup schema check timed out")?
        .map_err(|_| "history cleanup schema incompatible")?;
    let counts = tokio::time::timeout(Duration::from_secs(15), async {
        let restores = PostgresRestore::new(pool.clone())
            .expire_counted(&tenant)
            .await
            .map_err(|_| "restore expiry cleanup failed")?;
        let (exports, export_leases) = PostgresExports::new(pool)
            .expire_counted(&tenant)
            .await
            .map_err(|_| "export expiry cleanup failed")?;
        Ok::<_, &'static str>(HistoryCleanupCounts {
            restores,
            exports,
            export_leases,
        })
    })
    .await
    .map_err(|_| "history cleanup timed out")??;
    Ok(counts)
}
