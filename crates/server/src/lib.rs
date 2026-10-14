//! Backend composition root, HTTP health surface, and protected metadata queries for `FleetIQ`.
//!
//! The crate owns process configuration, PostgreSQL startup, migrations,
//! transport binding, and graceful shutdown. Browser sessions or configured
//! workload credentials authenticate metadata queries against current identity
//! mappings and tenant policy.

mod bearer_authentication;
mod bootstrap;
mod browser_identity;
mod command;
mod config;
#[cfg(test)]
mod contract_test_support;
mod health;
mod http_api;
mod operator_tenants;
mod protected_asset;
mod protected_definitions;
mod protected_device;
mod protected_relationship;
mod protected_request;
mod protected_signal_binding;

use std::{error::Error, fmt, io};

use axum::Router;
pub use bootstrap::{
    PlatformBootstrapCommandError, PlatformBootstrapConfig, PlatformBootstrapConfigError,
    bootstrap_platform_operator,
};
pub use browser_identity::{BrowserIdentityConfig, BrowserIdentityConfigError};
pub use command::{ProcessCommand, ProcessCommandError};
pub use config::{ServerConfig, ServerConfigError};
use fleetiq_postgres::MIGRATOR;
pub use health::health_router;
pub use http_api::api_router;
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::net::TcpListener;

/// Starts the configured backend and serves requests until shutdown.
///
/// Startup establishes the PostgreSQL pool and applies the complete embedded
/// migration chain before binding the listener. The server therefore never
/// advertises readiness against an older schema. Receiving `Ctrl+C`, or
/// `SIGTERM` on Unix, initiates Axum's graceful shutdown and then closes the
/// connection pool.
///
/// # Errors
///
/// Returns [`ServerError`] when PostgreSQL connection or migration, listener
/// binding, trusted OIDC discovery, or HTTP serving fails.
pub async fn run(config: ServerConfig) -> Result<(), ServerError> {
    let pool = PgPoolOptions::new()
        .connect(config.database_url())
        .await
        .map_err(ServerError::DatabaseConnection)?;
    MIGRATOR.run(&pool).await.map_err(ServerError::Migration)?;

    let application = application_router(pool.clone(), &config).await?;
    let listener = TcpListener::bind(config.bind_address())
        .await
        .map_err(ServerError::Listener)?;
    // JoinSet owns cancellation: dropping run() aborts the worker instead of
    // detaching a task that retains the PostgreSQL pool. Normal shutdown also
    // joins the aborted worker before closing database connections.
    let mut cleanup = tokio::task::JoinSet::new();
    if config.browser_identity().is_some() {
        let repository = fleetiq_postgres::PostgresBrowserIdentityRepository::new(pool.clone());
        cleanup.spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                let Ok(now) = browser_identity::now() else {
                    continue;
                };
                if repository.delete_expired(now).await.is_err() {
                    eprintln!("expired browser authentication records could not be removed");
                }
            }
        });
    }
    let serving = axum::serve(listener, application).with_graceful_shutdown(shutdown_signal());
    let result = serving.await.map_err(ServerError::Serving);
    cleanup.shutdown().await;
    pool.close().await;
    result
}

/// Selects one HTTP authentication mode at the composition boundary.
async fn application_router(pool: PgPool, config: &ServerConfig) -> Result<Router, ServerError> {
    let mut router = Router::new().merge(health_router(pool.clone()));
    if let Some(browser) = config.browser_identity() {
        let provider = browser_identity::OidcClient::discover(browser.clone())
            .await
            .map_err(|_| ServerError::BrowserIdentityStartup)?;
        router = router
            .merge(protected_read_router(
                pool.clone(),
                browser_identity::BrowserRequestAuthenticator::new(pool.clone(), browser),
            ))
            .merge(browser_identity::postgres_browser_router(
                pool,
                browser.clone(),
                provider,
            ));
    } else if let Some(bearer) = config.bearer_identity() {
        let authentication = fleetiq_access::CallerAuthenticationService::new(
            bearer_authentication::ConfiguredBearerAuthenticator::new(bearer.clone()),
            fleetiq_postgres::PostgresActorIdentityResolver::new(pool.clone()),
        );
        router = router.merge(protected_read_router(pool, authentication));
    }
    Ok(router.merge(api_router()))
}

/// Composes protected reads with one authentication strategy and fresh policy.
fn protected_read_router<A>(pool: PgPool, authentication: A) -> Router
where
    A: protected_request::RequestAuthenticator + Clone + 'static,
{
    Router::new()
        .merge(operator_tenants::postgres_operator_tenants_router(
            pool.clone(),
            authentication.clone(),
        ))
        .merge(protected_asset::postgres_asset_router(
            pool.clone(),
            authentication.clone(),
        ))
        .merge(protected_definitions::postgres_definition_router(
            pool.clone(),
            authentication.clone(),
        ))
        .merge(protected_device::postgres_device_router(
            pool.clone(),
            authentication.clone(),
        ))
        .merge(protected_relationship::postgres_relationship_router(
            pool.clone(),
            authentication.clone(),
        ))
        .merge(protected_signal_binding::postgres_signal_binding_router(
            pool,
            authentication,
        ))
}

async fn shutdown_signal() {
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
#[derive(Debug)]
pub enum ServerError {
    /// PostgreSQL could not be reached with the configured connection string.
    DatabaseConnection(sqlx::Error),
    /// The embedded PostgreSQL migration chain could not be applied.
    Migration(sqlx::migrate::MigrateError),
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
            Self::DatabaseConnection(error) => {
                write!(formatter, "failed to connect to PostgreSQL: {error}")
            }
            Self::Migration(error) => {
                write!(formatter, "failed to apply PostgreSQL migrations: {error}")
            }
            Self::BrowserIdentityStartup => {
                formatter.write_str("browser identity provider could not be initialized")
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
            Self::BrowserIdentityStartup => None,
            Self::Migration(error) => Some(error),
            Self::Listener(error) | Self::Serving(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn configured_process_router_mounts_protected_metadata_queries() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .expect("lazy test pool");
        let base = ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
            .expect("valid base configuration");
        let paths = [
            "/api/v1/me/tenants",
            "/api/v1/tenants/tenant-a/assets/asset-1",
            "/api/v1/tenants/tenant-a/assets/asset-1/device",
            "/api/v1/tenants/tenant-a/asset-types/generic.power-system/versions/2",
            "/api/v1/tenants/tenant-a/properties/thermal.temperature/versions/1",
            "/api/v1/tenants/tenant-a/relationship-types/generic.tows/versions/2",
            "/api/v1/tenants/tenant-a/relationships/relationship-1/revisions/1",
            "/api/v1/tenants/tenant-a/signal-bindings/binding-1/revisions/1",
        ];
        let unconfigured = application_router(pool.clone(), &base).await.unwrap();
        for path in paths {
            let response = unconfigured
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        }

        let configured = base
            .with_bearer_identity("a".repeat(64), "workload", "reader")
            .expect("valid bearer identity");
        let router = application_router(pool, &configured).await.unwrap();
        for path in paths {
            let response = router
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        }
        let response = router
            .oneshot(Request::get("/api/v1").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
