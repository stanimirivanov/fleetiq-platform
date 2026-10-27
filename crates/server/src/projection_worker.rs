//! Explicit checked-schema projector/rebuild process modes; no HTTP or DDL.
use fleetiq_domain::TenantId;
use fleetiq_postgres::{PostgresCurrentStateRepository, PostgresRepositoryError};
use fleetiq_projection::ProjectionMaintenance;
use sqlx::postgres::PgPoolOptions;
use std::{error::Error, fmt, time::Duration};
/// Runs bounded durable current-state batches until ordinary shutdown.
///
/// # Errors
/// Returns redacted startup errors. Failed turns retry durable work; an ambiguous
/// commit may already have completed the batch.
/// the timeout cancels the transaction rather than exposing partial revisions.
pub async fn run_projection_worker() -> Result<(), ProjectionRunError> {
    run(false).await
}
/// Rebuilds the active tenant named by `FLEETIQ_PROJECTION_REBUILD_TENANT` and exits.
///
/// # Errors
/// Returns a redacted configuration/schema/storage failure; failed rebuilds
/// retain the prior generation. This is a trusted database-operator command,
/// not an HTTP permission or an automatic action on metadata corrections.
pub async fn rebuild_current_state() -> Result<(), ProjectionRunError> {
    run(true).await
}
async fn run(rebuild: bool) -> Result<(), ProjectionRunError> {
    let url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .ok_or(ProjectionRunError::Configuration)?;
    let tenant = if rebuild {
        Some(
            TenantId::new(
                std::env::var("FLEETIQ_PROJECTION_REBUILD_TENANT")
                    .map_err(|_| ProjectionRunError::Configuration)?,
            )
            .map_err(|_| ProjectionRunError::Configuration)?,
        )
    } else {
        None
    };
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&url)
        .await
        .map_err(ProjectionRunError::Database)?;
    let result = async {
        crate::SchemaMode::Check
            .prepare(&pool)
            .await
            .map_err(ProjectionRunError::Schema)?;
        let repository = PostgresCurrentStateRepository::new(pool.clone());
        if let Some(tenant) = tenant {
            tokio::time::timeout(Duration::from_secs(8), repository.rebuild(&tenant))
                .await
                .map_err(|_| ProjectionRunError::Deadline)?
                .map_err(ProjectionRunError::Repository)?;
            println!("current-state rebuild committed");
            return Ok(());
        }
        let shutdown = crate::runtime::shutdown_signal();
        tokio::pin!(shutdown);
        loop {
            let turn = tokio::select! {
                biased;
                () = &mut shutdown => break,
                result = tokio::time::timeout(Duration::from_secs(8), repository.tick()) => result,
            };
            if !matches!(turn, Ok(Ok(_))) {
                eprintln!("current-state projector turn failed; retrying durable work");
            }
            tokio::select! {
                biased;
                () = &mut shutdown => break,
                () = tokio::time::sleep(Duration::from_millis(250)) => {},
            }
        }
        Ok(())
    }
    .await;
    pool.close().await;
    result
}
/// Redacted process failure; trusted callers can inspect the original cause.
pub enum ProjectionRunError {
    /// Missing or invalid private runtime configuration.
    Configuration,
    /// Pool connection failed.
    Database(sqlx::Error),
    /// The exact migration inventory is not installed.
    Schema(crate::SchemaPreparationError),
    /// The atomic rebuild failed; previous state remains intact.
    Repository(PostgresRepositoryError),
    /// The bounded rebuild deadline elapsed.
    Deadline,
}
impl fmt::Display for ProjectionRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "current-state configuration is missing or invalid",
            Self::Database(_) => "current-state database connection failed",
            Self::Schema(_) => "current-state schema check failed",
            Self::Repository(_) => "current-state atomic rebuild failed",
            Self::Deadline => "current-state rebuild deadline elapsed",
        })
    }
}
impl fmt::Debug for ProjectionRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for ProjectionRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(e) => Some(e),
            Self::Schema(e) => Some(e),
            Self::Repository(e) => Some(e),
            Self::Configuration | Self::Deadline => None,
        }
    }
}
