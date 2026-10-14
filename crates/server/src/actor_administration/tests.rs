//! HTTP integration tests use real identity policy, transactions, and current mappings.
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
const ACTORS: &str = "/api/v1/admin/actors";
const AUDIT: &str = "/api/v1/admin/actor-lifecycle-changes";
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
    let config = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "operator")
        .unwrap();
    let authentication = CallerAuthenticationService::new(
        crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
            config.bearer_identity().unwrap().clone(),
        ),
        PostgresActorIdentityResolver::new(pool.clone()),
    );
    postgres_actor_administration_router(pool.clone(), authentication, MutationGuard::Workload)
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
fn provision(id: &str, actor: &str) -> Value {
    json!({"provisioning_id":id,"actor_id":actor,"external_identity":{"authority_id":"workforce","subject":format!("subject-{actor}")}})
}
fn lifecycle(id: &str, state: &str) -> Value {
    json!({"change_id":id,"state":state})
}
async fn body(response: Response) -> Value {
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    serde_json::from_slice(&to_bytes(response.into_body(), 32768).await.unwrap()).unwrap()
}
