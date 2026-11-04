//! Process resources and cancellation, separate from HTTP route composition.
use crate::{
    SchemaMode, SchemaPreparationError, ServerConfig, application_router, protected_request,
};
use sqlx::postgres::PgPoolOptions;
use std::{error::Error, fmt, future::Future, io, time::Duration};
use tokio::{net::TcpListener, sync::oneshot, time::Instant};

// Leave time for process teardown before the local supervisor's 10s grace.
const HTTP_SHUTDOWN_GRACE: Duration = Duration::from_secs(7);

/// Starts the configured backend and serves requests until shutdown.
///
/// Startup establishes the PostgreSQL pool and prepares the schema according
/// to the explicit mode before binding the listener. The server therefore never
/// advertises readiness against an older schema. Receiving `Ctrl+C`, or
/// `SIGTERM` on Unix, initiates a bounded graceful drain and connection pool shutdown.
///
/// # Errors
///
/// Returns [`ServerError`] when PostgreSQL connection or migration, listener
/// binding, trusted OIDC discovery, or HTTP serving fails, or the drain expires.
pub async fn run(config: ServerConfig) -> Result<(), ServerError> {
    run_until(config, shutdown_signal()).await
}

/// Runs with an owner-provided shutdown future for deterministic lifecycle verification.
///
/// Schema/identity validation still precede listening. Once shutdown resolves,
/// the drain, browser cleanup and pool close share one deadline. The executable
/// terminates the process if that deadline expires.
///
/// # Errors
/// Returns the same redacted startup/serving errors as [`run`].
pub(crate) async fn run_until(
    config: ServerConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServerError> {
    if config.schema_mode() == SchemaMode::Migrate {
        // Migration locks and DDL have their own lifecycle. Never run them on
        // serving connections or carry migration session settings into HTTP.
        let migration_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(config.database_url())
            .await
            .map_err(ServerError::DatabaseConnection)?;
        let prepared = SchemaMode::Migrate.prepare(&migration_pool).await;
        migration_pool.close().await;
        prepared.map_err(ServerError::Schema)?;
    }

    let pool = PgPoolOptions::new()
        .acquire_timeout(Duration::from_secs(3))
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET statement_timeout = '5s'")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(config.database_url())
        .await
        .map_err(ServerError::DatabaseConnection)?;
    let startup = async {
        SchemaMode::Check
            .prepare(&pool)
            .await
            .map_err(ServerError::Schema)?;
        let application = application_router(pool.clone(), &config).await?;
        let listener = TcpListener::bind(config.bind_address())
            .await
            .map_err(ServerError::Listener)?;
        Ok((listener, application))
    }
    .await;

    match startup {
        Ok((listener, application)) => {
            serve_listener(
                listener,
                application,
                pool,
                config.browser_identity().is_some(),
                shutdown,
            )
            .await
        }
        Err(error) => {
            pool.close().await;
            Err(error)
        }
    }
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
    let (deadline_sender, deadline_receiver) = oneshot::channel();
    let serving = axum::serve(listener, application).with_graceful_shutdown(async move {
        shutdown.await;
        let _ = deadline_sender.send(Instant::now() + HTTP_SHUTDOWN_GRACE);
    });
    let graceful = async {
        let result = serving.await.map_err(ServerError::Serving);
        cleanup.shutdown().await;
        pool.close().await;
        result
    };
    let deadline = async {
        match deadline_receiver.await {
            Ok(deadline) => tokio::time::sleep_until(deadline).await,
            // No shutdown signal was observed. Let the serving result win.
            Err(_) => std::future::pending::<()>().await,
        }
    };
    tokio::select! {
        biased;
        () = deadline => Err(ServerError::ShutdownDeadline),
        result = graceful => result,
    }
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
    /// The HTTP drain and cleanup exceeded their shared grace period.
    ShutdownDeadline,
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
            Self::ShutdownDeadline => formatter.write_str("HTTP shutdown deadline exceeded"),
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
            Self::BrowserIdentityStartup | Self::ShutdownDeadline => None,
            Self::Schema(error) => Some(error),
            Self::Listener(error) | Self::Serving(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests;
