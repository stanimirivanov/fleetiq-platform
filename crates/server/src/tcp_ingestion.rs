//! Explicit TCP composition; HTTP and historical replay remain separate modes.
mod config;
mod executor;
#[cfg(test)]
mod tests;
mod transport;
pub use config::TcpIngestionConfig;
use fleetiq_ingestion::ProtocolRoutingService;
use fleetiq_postgres::{PostgresProtocolRoutingRepository, PostgresReplayRepository};
use sqlx::postgres::PgPoolOptions;
use std::{error::Error, fmt, time::Duration};
/// Runs the bounded identifier-only listener until SIGINT/SIGTERM.
/// # Errors
/// Fails closed on unsupported isolation, configuration, schema, admission or
/// listener failure. It never applies migrations or starts HTTP. Session errors
/// close only their own connection and never log payload/identity/peer values.
pub async fn run_tcp_ingestion(config: TcpIngestionConfig) -> Result<(), TcpIngestionError> {
    if !cfg!(target_os = "linux") {
        return Err(TcpIngestionError::category(
            "TCP ingestion requires Linux isolation",
        ));
    }
    executor::check_installed()
        .map_err(|e| TcpIngestionError::with_source("installed live executor unavailable", e))?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&config.database_url)
        .await
        .map_err(|e| TcpIngestionError::with_source("TCP database connection failed", e))?;
    let execution = executor::IsolatedLive::new(pool.clone());
    let result = async {
        crate::SchemaMode::Check
            .prepare(&pool)
            .await
            .map_err(|e| TcpIngestionError::with_source("TCP schema check failed", e))?;
        let routing = PostgresProtocolRoutingRepository::new(pool.clone());
        if ProtocolRoutingService::new(routing.clone())
            .bootstrap(&config.channel)
            .await
            .map_err(|e| TcpIngestionError::with_source("TCP channel lookup failed", e))?
            .is_none()
        {
            return Err(TcpIngestionError::category(
                "TCP channel not currently admitted",
            ));
        }
        let listener = tokio::net::TcpListener::bind(config.bind)
            .await
            .map_err(|e| TcpIngestionError::with_source("TCP listener bind failed", e))?;
        transport::listen(
            listener,
            config.channel,
            routing,
            PostgresReplayRepository::new(pool.clone()),
            execution.clone(),
            crate::runtime::shutdown_signal(),
        )
        .await
        .map_err(|e| TcpIngestionError::with_source("TCP listener failed", e))
    }
    .await;
    execution.drain().await;
    pool.close().await;
    result
}
/// Sanitized process failure; optional private causes are never rendered by Debug.
pub struct TcpIngestionError {
    category: &'static str,
    source: Option<Box<dyn Error + Send + Sync>>,
}
impl TcpIngestionError {
    fn category(category: &'static str) -> Self {
        Self {
            category,
            source: None,
        }
    }
    fn with_source(category: &'static str, source: impl Error + Send + Sync + 'static) -> Self {
        Self {
            category,
            source: Some(Box::new(source)),
        }
    }
}
impl fmt::Display for TcpIngestionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.category)
    }
}
impl fmt::Debug for TcpIngestionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for TcpIngestionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|s| s.as_ref() as &(dyn Error + 'static))
    }
}
