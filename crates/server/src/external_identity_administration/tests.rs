//! Real policy and PostgreSQL tests for the composed identity administration boundary.
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
const MAPPINGS: &str = "/api/v1/admin/actors/actor-b/external-identities";
const COMMANDS: &str = "/api/v1/admin/actors/actor-b/external-identity-changes";
const AUDIT: &str = "/api/v1/admin/external-identity-changes";

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
    let app =
        crate::protected_application_router(pool.clone(), authentication, MutationGuard::Workload);
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/admin/actors",
            &json!({
                "provisioning_id":"provision-b","actor_id":"actor-b",
                "external_identity":{"authority_id":"workforce","subject":"subject-actor-b"}
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    app
}
fn request(method: &str, path: &str, payload: &Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap()
}
fn change(id: &str, kind: &str, authority: &str, subject: &str) -> Value {
    json!({"change_id":id,"kind":kind,"external_identity":{"authority_id":authority,"subject":subject}})
}
async fn body(response: Response) -> Value {
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    serde_json::from_slice(&to_bytes(response.into_body(), 1_048_576).await.unwrap()).unwrap()
}
async fn send(app: &Router, method: &str, path: &str, payload: &Value) -> Value {
    let response = app
        .clone()
        .oneshot(request(method, path, payload))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    body(response).await
}
fn continuation(path: &str, pairs: &[(&str, &str)]) -> String {
    format!(
        "{path}?{}",
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(pairs.iter().copied())
            .finish()
    )
}
async fn audit_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM identity.external_identity_changes")
        .fetch_one(pool)
        .await
        .unwrap()
}
