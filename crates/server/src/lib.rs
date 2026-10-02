//! Backend composition root, HTTP health surface, and protected metadata queries for `FleetIQ`.
//!
//! The crate owns process configuration, PostgreSQL startup, migrations,
//! transport binding, and graceful shutdown. Configured-credential asset,
//! device, and definition queries use current tenant policy and metadata lookup.

mod bearer_authentication;
mod bootstrap;
mod command;
mod config;
mod health;
mod http_api;
mod protected_asset;
mod protected_definitions;
mod protected_device;
mod protected_relationship;
mod protected_request;

use std::{error::Error, fmt, io};

use axum::Router;
pub use bootstrap::{
    PlatformBootstrapCommandError, PlatformBootstrapConfig, PlatformBootstrapConfigError,
    bootstrap_platform_operator,
};
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
    let application = application_router(pool.clone(), &config);
    let serving = axum::serve(listener, application).with_graceful_shutdown(shutdown_signal());
    let result = serving.await.map_err(ServerError::Serving);
    pool.close().await;
    result
}

fn application_router(pool: PgPool, config: &ServerConfig) -> Router {
    let mut router = Router::new().merge(health_router(pool.clone()));
    if let Some(bearer) = config.bearer_identity() {
        router = router
            .merge(protected_asset::postgres_asset_router(
                pool.clone(),
                bearer.clone(),
            ))
            .merge(protected_definitions::postgres_definition_router(
                pool.clone(),
                bearer.clone(),
            ))
            .merge(protected_device::postgres_device_router(
                pool.clone(),
                bearer.clone(),
            ))
            .merge(protected_relationship::postgres_relationship_router(
                pool,
                bearer.clone(),
            ));
    }
    router.merge(api_router())
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
        let unconfigured = application_router(pool.clone(), &base);
        let path = "/api/v1/tenants/tenant-a/assets/asset-1";
        let device_path = "/api/v1/tenants/tenant-a/assets/asset-1/device";
        let type_path = "/api/v1/tenants/tenant-a/asset-types/generic.power-system/versions/2";
        let property_path = "/api/v1/tenants/tenant-a/properties/thermal.temperature/versions/1";
        let relationship_type_path =
            "/api/v1/tenants/tenant-a/relationship-types/generic.tows/versions/2";
        let relationship_path = "/api/v1/tenants/tenant-a/relationships/relationship-1/revisions/1";
        let response = unconfigured
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = application_router(pool.clone(), &base)
            .oneshot(Request::get(relationship_path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = application_router(pool.clone(), &base)
            .oneshot(
                Request::get(relationship_type_path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = application_router(pool.clone(), &base)
            .oneshot(Request::get(property_path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = application_router(pool.clone(), &base)
            .oneshot(Request::get(type_path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = application_router(pool.clone(), &base)
            .oneshot(Request::get(device_path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let configured = base
            .with_bearer_identity("a".repeat(64), "workload", "reader")
            .expect("valid bearer identity");
        let router = application_router(pool, &configured);
        let response = router
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = router
            .clone()
            .oneshot(Request::get(relationship_path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = router
            .clone()
            .oneshot(
                Request::get(relationship_type_path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = router
            .clone()
            .oneshot(Request::get(property_path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = router
            .clone()
            .oneshot(Request::get(type_path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = router
            .clone()
            .oneshot(Request::get(device_path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = router
            .oneshot(Request::get("/api/v1").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
