//! Process resources and cancellation, separate from HTTP route composition.
use crate::{SchemaPreparationError, ServerConfig, application_router, protected_request};
use sqlx::postgres::PgPoolOptions;
use std::{error::Error, fmt, future::Future, io};
use tokio::net::TcpListener;

/// Starts the configured backend and serves requests until shutdown.
///
/// Startup establishes the PostgreSQL pool and prepares the schema according
/// to the explicit mode before binding the listener. The server therefore never
/// advertises readiness against an older schema. Receiving `Ctrl+C`, or
/// `SIGTERM` on Unix, initiates Axum's graceful shutdown and then closes the
/// connection pool.
///
/// # Errors
///
/// Returns [`ServerError`] when PostgreSQL connection or migration, listener
/// binding, trusted OIDC discovery, or HTTP serving fails.
pub async fn run(config: ServerConfig) -> Result<(), ServerError> {
    run_until(config, shutdown_signal()).await
}

/// Runs with an owner-provided shutdown future for deterministic lifecycle verification.
///
/// Schema/identity validation still precede listening. Once shutdown resolves,
/// in-flight HTTP requests drain, the browser cleanup worker is joined, and the
/// pool is closed. A supervisor must enforce its external termination deadline;
/// this function does not forcibly roll back or abandon an in-flight command.
///
/// # Errors
/// Returns the same redacted startup/serving errors as [`run`].
pub(crate) async fn run_until(
    config: ServerConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServerError> {
    let pool = PgPoolOptions::new()
        .connect(config.database_url())
        .await
        .map_err(ServerError::DatabaseConnection)?;
    let result = async {
        config
            .schema_mode()
            .prepare(&pool)
            .await
            .map_err(ServerError::Schema)?;
        let application = application_router(pool.clone(), &config).await?;
        let listener = TcpListener::bind(config.bind_address())
            .await
            .map_err(ServerError::Listener)?;
        serve_listener(
            listener,
            application,
            pool.clone(),
            config.browser_identity().is_some(),
            shutdown,
        )
        .await
    }
    .await;
    pool.close().await;
    result
}

async fn serve_listener(
    listener: TcpListener,
    application: axum::Router,
    pool: sqlx::PgPool,
    browser_cleanup: bool,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServerError> {
    // JoinSet owns cancellation: dropping run() aborts the worker instead of
    // detaching a task that retains the PostgreSQL pool. Normal shutdown also
    // joins the aborted worker before closing database connections.
    let mut cleanup = tokio::task::JoinSet::new();
    if browser_cleanup {
        let repository = fleetiq_postgres::PostgresBrowserIdentityRepository::new(pool.clone());
        cleanup.spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                let Ok(now) = protected_request::now() else {
                    continue;
                };
                if repository.delete_expired(now).await.is_err() {
                    eprintln!("expired browser authentication records could not be removed");
                }
            }
        });
    }
    let serving = axum::serve(listener, application).with_graceful_shutdown(shutdown);
    let result = serving.await.map_err(ServerError::Serving);
    cleanup.shutdown().await;
    pool.close().await;
    result
}

pub(crate) async fn shutdown_signal() {
    let interrupt = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            eprintln!("failed to install or receive the interrupt signal: {error}");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    {
        let terminate = async {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut signal) => {
                    signal.recv().await;
                }
                Err(error) => {
                    eprintln!("failed to install the termination signal: {error}");
                    std::future::pending::<()>().await;
                }
            }
        };

        tokio::select! {
            () = interrupt => {},
            () = terminate => {},
        }
    }

    #[cfg(not(unix))]
    {
        interrupt.await;
    }
}

/// Failure while starting or serving the backend.
pub enum ServerError {
    /// PostgreSQL could not be reached with the configured connection string.
    DatabaseConnection(sqlx::Error),
    /// The embedded PostgreSQL migration chain could not be applied.
    Schema(SchemaPreparationError),
    /// Trusted OIDC discovery or HTTP-client setup failed before serving.
    BrowserIdentityStartup,
    /// The configured network listener could not be bound.
    Listener(io::Error),
    /// The HTTP server terminated with an I/O failure.
    Serving(io::Error),
}

impl fmt::Display for ServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DatabaseConnection(_) => formatter.write_str("failed to connect to PostgreSQL"),
            Self::Schema(error) => {
                write!(formatter, "database schema is not ready: {error}")
            }
            Self::BrowserIdentityStartup => {
                formatter.write_str("browser identity provider could not be initialized")
            }
            Self::Listener(error) => write!(formatter, "failed to bind HTTP listener: {error}"),
            Self::Serving(error) => write!(formatter, "HTTP server failed: {error}"),
        }
    }
}

impl fmt::Debug for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Error for ServerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::DatabaseConnection(error) => Some(error),
            Self::BrowserIdentityStartup => None,
            Self::Schema(error) => Some(error),
            Self::Listener(error) | Self::Serving(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests;
