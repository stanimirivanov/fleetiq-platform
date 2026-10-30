//! Backend composition root, HTTP health surface, and protected identity/metadata operations for `FleetIQ`.
//!
//! The crate owns process configuration, PostgreSQL startup, migrations,
//! transport binding, and graceful shutdown. Browser sessions or configured
//! workload credentials authenticate protected operations against current identity
//! mappings and tenant policy.

mod actor_administration;
mod bearer_authentication;
mod bootstrap;
mod browser_identity;
mod command;
mod current_state;
mod fleet;
mod live_updates;
mod projection_worker;
pub use projection_worker::{ProjectionRunError, rebuild_current_state, run_projection_worker};
mod config;
#[cfg(test)]
mod contract_test_support;
mod definition_administration;
mod external_identity_administration;
mod health;
mod http_api;
mod migration;
mod runtime;
mod schema_mode;
pub use runtime::{ServerError, run};
mod ingestion_diagnostics;
mod mutation_guard;
mod operator_tenants;
mod platform_access_administration;
mod property_history;
mod protected_asset;
mod protected_definitions;
mod protected_device;
mod protected_relationship;
mod protected_request;
mod protected_signal_binding;
mod protocol_packages;
mod protocol_routes;
mod registration;
mod revision_commands;
mod semantic_graph;
mod tenant_access_administration;
mod tenant_administration;

use axum::Router;
pub use bootstrap::{
    PlatformBootstrapCommandError, PlatformBootstrapConfig, PlatformBootstrapConfigError,
    bootstrap_platform_operator,
};
pub use browser_identity::{BrowserIdentityConfig, BrowserIdentityConfigError};
pub use command::{ProcessCommand, ProcessCommandError};
pub use config::{ServerConfig, ServerConfigError};
pub use health::health_router;
pub use http_api::api_router;
pub use migration::{
    MigrationCommandError, MigrationConfig, migrate_database, migration_inventory,
};
pub use schema_mode::{SchemaMode, SchemaPreparationError};
use sqlx::PgPool;

/// Selects one HTTP authentication mode at the composition boundary.
async fn application_router(pool: PgPool, config: &ServerConfig) -> Result<Router, ServerError> {
    let mut router = Router::new().merge(health_router(pool.clone()));
    if let Some(browser) = config.browser_identity() {
        let provider = browser_identity::OidcClient::discover(browser.clone())
            .await
            .map_err(|_| ServerError::BrowserIdentityStartup)?;
        router = router
            .merge(protected_application_router(
                pool.clone(),
                browser_identity::BrowserRequestAuthenticator::new(pool.clone(), browser),
                mutation_guard::MutationGuard::Browser {
                    public_origin: browser.public_origin().to_owned(),
                },
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
        router = router.merge(protected_application_router(
            pool,
            authentication,
            mutation_guard::MutationGuard::Workload,
        ));
    }
    Ok(router.merge(api_router()))
}

/// Composes protected operations with one authentication strategy and fresh policy.
fn protected_application_router<A>(
    pool: PgPool,
    authentication: A,
    mutation: mutation_guard::MutationGuard,
) -> Router
where
    A: protected_request::RequestAuthenticator + Clone + 'static,
{
    Router::new()
        .merge(fleet::router(pool.clone(), authentication.clone()))
        .merge(live_updates::router(pool.clone(), authentication.clone()))
        .merge(current_state::router(pool.clone(), authentication.clone()))
        .merge(semantic_graph::router(pool.clone(), authentication.clone()))
        .merge(property_history::router(
            pool.clone(),
            authentication.clone(),
        ))
        .merge(ingestion_diagnostics::router(
            pool.clone(),
            authentication.clone(),
        ))
        .merge(protocol_routes::router(
            pool.clone(),
            authentication.clone(),
            mutation.clone(),
        ))
        .merge(protocol_packages::router(
            pool.clone(),
            authentication.clone(),
            mutation.clone(),
        ))
        .merge(revision_commands::postgres_revision_router(
            pool.clone(),
            authentication.clone(),
            mutation.clone(),
        ))
        .merge(registration::postgres_registration_router(
            pool.clone(),
            authentication.clone(),
            mutation.clone(),
        ))
        .merge(
            definition_administration::postgres_definition_administration_router(
                pool.clone(),
                authentication.clone(),
                mutation.clone(),
            ),
        )
        .merge(
            tenant_access_administration::postgres_tenant_access_administration_router(
                pool.clone(),
                authentication.clone(),
                mutation.clone(),
            ),
        )
        .merge(
            tenant_administration::postgres_tenant_administration_router(
                pool.clone(),
                authentication.clone(),
                mutation.clone(),
            ),
        )
        .merge(
            platform_access_administration::postgres_platform_access_administration_router(
                pool.clone(),
                authentication.clone(),
                mutation.clone(),
            ),
        )
        .merge(actor_administration::postgres_actor_administration_router(
            pool.clone(),
            authentication.clone(),
            mutation.clone(),
        ))
        .merge(
            external_identity_administration::postgres_external_identity_administration_router(
                pool.clone(),
                authentication.clone(),
                mutation,
            ),
        )
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

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use sqlx::postgres::PgPoolOptions;
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
            "/api/v1/tenants/tenant-a/assets/device-a/ingestion-diagnostics",
            "/api/v1/me/tenants",
            "/api/v1/tenants/tenant-a/protocol-packages/custom.binary/versions/1",
            "/api/v1/tenants/tenant-a/assets/device-a/protocol-routes/tcp-a",
            "/api/v1/admin/actors",
            "/api/v1/admin/actors/actor-a/platform-grants",
            "/api/v1/admin/platform-access-changes",
            "/api/v1/admin/actor-lifecycle-changes",
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

mod replay_worker;
pub use replay_worker::{ReplayWorkerError, run_replay_worker};

mod protocol_process;
mod tcp_ingestion;
pub use tcp_ingestion::{TcpIngestionConfig, TcpIngestionError, run_tcp_ingestion};

mod semantic_worker;
pub use semantic_worker::{SemanticWorkerRunError, run_semantic_worker};

mod calculator_registration;
pub use calculator_registration::register_calculator_definition;
