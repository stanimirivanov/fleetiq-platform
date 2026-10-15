//! Real PostgreSQL integration at the HTTP boundary, including current policy.
mod authorization;
mod commands;
mod queries;
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use fleetiq_access::CallerAuthenticationService;
use fleetiq_domain::{
    ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId, UnixTimeMillis,
};
use fleetiq_identity::{PlatformBootstrapRequest, PlatformBootstrapService};
use fleetiq_postgres::{PostgresActorIdentityResolver, PostgresPlatformBootstrap};
use serde_json::{Value, json};
use tower::ServiceExt;
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TENANTS: &str = "/api/v1/admin/tenants";
const AUDIT: &str = "/api/v1/admin/tenant-lifecycle-changes";
const LIFECYCLE: &str = "/api/v1/admin/tenants/tenant-a/lifecycle";
async fn setup(pool: &PgPool) -> Router {
    PlatformBootstrapService::new(PostgresPlatformBootstrap::new(pool.clone()))
        .bootstrap(&PlatformBootstrapRequest::new(
            ActorId::new("operator-a").unwrap(),
            ExternalIdentity::new(
                IdentityAuthorityId::new("workload").unwrap(),
                ExternalIdentitySubject::new("operator").unwrap(),
            ),
            UnixTimeMillis::new(100),
        ))
        .await
        .unwrap();
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ('admin-a'), ('admin-b')")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO identity.external_actor_identities (authority_id, subject, actor_id) VALUES ('workload', 'administrator', 'admin-a')").execute(pool).await.unwrap();
    router(pool, "operator")
}
fn router(pool: &PgPool, subject: &str) -> Router {
    let config = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", subject)
        .unwrap();
    let authentication = CallerAuthenticationService::new(
        crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
            config.bearer_identity().unwrap().clone(),
        ),
        PostgresActorIdentityResolver::new(pool.clone()),
    );
    crate::protected_application_router(pool.clone(), authentication, MutationGuard::Workload)
}
fn request(method: &str, path: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}
fn provision(id: &str, tenant: &str) -> Value {
    json!({"provisioning_id":id,"tenant_id":tenant,"initial_administrator_actor_id":"admin-a"})
}
fn lifecycle(id: &str, state: &str) -> Value {
    json!({"change_id":id,"state":state})
}
async fn body(response: Response) -> Value {
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    serde_json::from_slice(&to_bytes(response.into_body(), 32768).await.unwrap()).unwrap()
}
async fn create(app: &Router, tenant: &str) {
    assert_eq!(
        app.clone()
            .oneshot(request("POST", TENANTS, &provision(tenant, tenant)))
            .await
            .unwrap()
            .status(),
        StatusCode::CREATED
    );
}
