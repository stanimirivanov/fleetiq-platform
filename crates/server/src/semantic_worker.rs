//! Checked-schema composition of the durable semantic worker; no HTTP or DDL.
use fleetiq_history::{SemanticTick, SemanticWorker};
use fleetiq_postgres::{
    PostgresDecodedObservationView, PostgresDefinitionRepository, PostgresSemanticWorkRepository,
    PostgresSignalBindingRepository,
};
use sqlx::postgres::PgPoolOptions;
use std::{error::Error, fmt, time::Duration};

/// Runs bounded semantic interpretation until an ordinary termination signal.
///
/// Requires only `DATABASE_URL`. Uses one in-flight claim, a four-connection
/// pool, five-second statements and a ten-second turn deadline. Cancellation
/// drops transactions; committed selections remain recoverable after expiry.
///
/// # Errors
/// Returns sanitized configuration, database connection or schema failures.
/// Individual turn failures retain work and retry only through fenced claims.
pub async fn run_semantic_worker() -> Result<(), SemanticWorkerRunError> {
    run_until(crate::runtime::shutdown_signal()).await
}
async fn run_until(
    shutdown: impl std::future::Future<Output = ()> + Send,
) -> Result<(), SemanticWorkerRunError> {
    let url = std::env::var("DATABASE_URL").map_err(|_| SemanticWorkerRunError::Configuration)?;
    if url.trim().is_empty() {
        return Err(SemanticWorkerRunError::Configuration);
    }
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(3))
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET statement_timeout = '5s'")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .map_err(SemanticWorkerRunError::Database)?;
    let result = async {
        crate::SchemaMode::Check
            .prepare(&pool)
            .await
            .map_err(SemanticWorkerRunError::Schema)?;
        let worker = SemanticWorker::new(
            PostgresSemanticWorkRepository::new(pool.clone()),
            PostgresDecodedObservationView::new(pool.clone()),
            PostgresSignalBindingRepository::new(pool.clone()),
            PostgresDefinitionRepository::new(pool.clone()),
        );
        tokio::pin!(shutdown);
        loop {
            let turn = tokio::select! {
                biased;
                ()=&mut shutdown => break,
                result=tokio::time::timeout(Duration::from_secs(10),worker.run_once()) => result,
            };
            if matches!(
                turn,
                Ok(Ok(SemanticTick::Completed | SemanticTick::Unresolved))
            ) {
                continue;
            }
            if !matches!(turn, Ok(Ok(SemanticTick::Idle | SemanticTick::Fenced))) {
                // No SQL/source tree, telemetry or credentials in routine output.
                eprintln!("semantic turn failed; durable work retained for fenced recovery");
            }
            tokio::select! {
                biased;
                ()=&mut shutdown => break,
                ()=tokio::time::sleep(Duration::from_millis(250)) => {},
            }
        }
        Ok(())
    }
    .await;
    pool.close().await;
    result
}
/// Redacted startup failure; the cause remains available to trusted callers.
pub enum SemanticWorkerRunError {
    /// Missing database configuration.
    Configuration,
    /// Database connection or fixed session configuration failed.
    Database(sqlx::Error),
    /// The exact embedded migrations are not installed.
    Schema(crate::SchemaPreparationError),
}
impl fmt::Display for SemanticWorkerRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "semantic worker requires DATABASE_URL",
            Self::Database(_) => "semantic worker database connection failed",
            Self::Schema(_) => "semantic worker schema check failed",
        })
    }
}
impl fmt::Debug for SemanticWorkerRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for SemanticWorkerRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Configuration => None,
            Self::Database(e) => Some(e),
            Self::Schema(e) => Some(e),
        }
    }
}
