//! Explicit worker composition: no HTTP listener or schema mutation.
mod executor;
use fleetiq_ingestion::{ReplayTick, ReplayWorker};
use fleetiq_postgres::PostgresReplayRepository;
use sqlx::postgres::PgPoolOptions;
use std::{error::Error, fmt, time::Duration};
/// Runs durable replay until a termination signal, using `DATABASE_URL` only.
/// # Errors
/// Returns sanitized configuration, schema or storage failures. Child failures
/// remain durable lease/poison state; shutdown drains child cleanup before exit.
pub async fn run_replay_worker() -> Result<(), ReplayWorkerError> {
    if !cfg!(target_os = "linux") {
        return Err(ReplayWorkerError::Configuration);
    }
    run_until(crate::runtime::shutdown_signal()).await
}
async fn run_until(
    shutdown: impl std::future::Future<Output = ()> + Send,
) -> Result<(), ReplayWorkerError> {
    let url = std::env::var("DATABASE_URL").map_err(|_| ReplayWorkerError::Configuration)?;
    if url.trim().is_empty() {
        return Err(ReplayWorkerError::Configuration);
    }
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .map_err(ReplayWorkerError::Database)?;
    let result = async {
        crate::SchemaMode::Check
            .prepare(&pool)
            .await
            .map_err(ReplayWorkerError::Schema)?;
        executor::check_installed().map_err(ReplayWorkerError::Executor)?;
        let repository = PostgresReplayRepository::new(pool.clone());
        let executor = executor::IsolatedReplay::new(pool.clone());
        let worker = ReplayWorker::new(&repository, &executor);
        tokio::pin!(shutdown);
        let result = loop {
            let tick = tokio::select! {
                biased;
                () = &mut shutdown => break Ok(()),
                result = worker.run_once() => result,
            };
            let tick = match tick {
                Ok(tick) => tick,
                Err(error) => break Err(ReplayWorkerError::Repository(error)),
            };
            if tick != ReplayTick::Completed {
                tokio::select! {
                    biased;
                    () = &mut shutdown => break Ok(()),
                    () = tokio::time::sleep(Duration::from_millis(250)) => {},
                }
            }
        };
        // Dropping an in-flight execute future cancels its supervised child.
        // Drain before closing the pool so shutdown cannot orphan guest work.
        executor.drain().await;
        result
    }
    .await;
    pool.close().await;
    result
}
/// Worker failure category; formatting never exposes configuration or raw SQL.
pub enum ReplayWorkerError {
    /// Missing configuration or an unsupported execution platform.
    Configuration,
    /// Database connection failed.
    Database(sqlx::Error),
    /// Exact migrated schema is unavailable.
    Schema(crate::SchemaPreparationError),
    /// Durable replay storage failed.
    Repository(fleetiq_postgres::PostgresReplayError),
    /// Fixed sibling executor is missing or not a regular file.
    Executor(std::io::Error),
}
impl fmt::Display for ReplayWorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "replay worker requires Linux and DATABASE_URL",
            Self::Database(_) => "replay worker database connection failed",
            Self::Schema(_) => "replay worker schema check failed",
            Self::Repository(_) => {
                "replay worker storage failed; retained lease remains recoverable"
            }
            Self::Executor(_) => "installed replay executor is unavailable",
        })
    }
}
impl fmt::Debug for ReplayWorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for ReplayWorkerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Configuration => None,
            Self::Database(e) => Some(e),
            Self::Schema(e) => Some(e),
            Self::Repository(e) => Some(e),
            Self::Executor(e) => Some(e),
        }
    }
}
