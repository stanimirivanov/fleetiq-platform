//! Backend composition root and HTTP health surface for `FleetIQ`.
//!
//! The crate owns process configuration, PostgreSQL startup, migrations,
//! transport binding, and graceful shutdown. Business routes will be composed
//! here as independently authorized application capabilities are implemented.

mod config;
mod health;

use std::{error::Error, fmt, io};

use axum::Router;
pub use config::{ServerConfig, ServerConfigError};
use fleetiq_postgres::MIGRATOR;
pub use health::health_router;
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::net::TcpListener;

/// Starts the configured backend and serves requests until shutdown.
///
/// Startup establishes the PostgreSQL pool and applies the complete embedded
/// migration chain before binding the listener. The server therefore never
/// advertises readiness against an older schema. Receiving `Ctrl+C` initiates
/// Axum's graceful shutdown and then closes the connection pool.
///
/// # Errors
///
/// Returns [`ServerError`] when PostgreSQL connection or migration, listener
/// binding, or HTTP serving fails.
pub async fn run(config: ServerConfig) -> Result<(), ServerError> {
    let pool = PgPoolOptions::new()
        .connect(config.database_url())
        .await
        .map_err(ServerError::DatabaseConnection)?;
    MIGRATOR.run(&pool).await.map_err(ServerError::Migration)?;

    let listener = TcpListener::bind(config.bind_address())
        .await
        .map_err(ServerError::Listener)?;
    let application = application_router(pool.clone());
    let serving = axum::serve(listener, application).with_graceful_shutdown(shutdown_signal());
    let result = serving.await.map_err(ServerError::Serving);
    pool.close().await;
    result
}

fn application_router(pool: PgPool) -> Router {
    Router::new().merge(health_router(pool))
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        eprintln!("failed to install or receive the shutdown signal: {error}");
    }
}

/// Failure while starting or serving the backend.
#[derive(Debug)]
pub enum ServerError {
    /// PostgreSQL could not be reached with the configured connection string.
    DatabaseConnection(sqlx::Error),
    /// The embedded PostgreSQL migration chain could not be applied.
    Migration(sqlx::migrate::MigrateError),
    /// The configured network listener could not be bound.
    Listener(io::Error),
    /// The HTTP server terminated with an I/O failure.
    Serving(io::Error),
}

impl fmt::Display for ServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DatabaseConnection(error) => {
                write!(formatter, "failed to connect to PostgreSQL: {error}")
            }
            Self::Migration(error) => {
                write!(formatter, "failed to apply PostgreSQL migrations: {error}")
            }
            Self::Listener(error) => write!(formatter, "failed to bind HTTP listener: {error}"),
            Self::Serving(error) => write!(formatter, "HTTP server failed: {error}"),
        }
    }
}

impl Error for ServerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::DatabaseConnection(error) => Some(error),
            Self::Migration(error) => Some(error),
            Self::Listener(error) | Self::Serving(error) => Some(error),
        }
    }
}
