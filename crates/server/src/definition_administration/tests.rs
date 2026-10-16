//! Real PostgreSQL and composed HTTP boundaries, with no mocked persistence.
mod authorization;
mod catalogue;
mod publication;
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use fleetiq_access::CallerAuthenticationService;
use fleetiq_postgres::PostgresActorIdentityResolver;
use serde_json::{Value, json};
use tower::ServiceExt;
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROOT: &str = "/api/v1/tenants/tenant-a";
async fn setup(pool: &PgPool) -> Router {
    sqlx::raw_sql("
        INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a'),('tenant-b');
        INSERT INTO identity.actors (actor_id) VALUES ('operator-a');
        INSERT INTO identity.external_actor_identities (authority_id,subject,actor_id) VALUES ('workload','operator','operator-a');
        INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES
          ('tenant-a','operator-a','metadata.read'),('tenant-a','operator-a','metadata.manage');
    ").execute(pool).await.unwrap();
    let config = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "operator")
        .unwrap();
    let auth = CallerAuthenticationService::new(
        crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
            config.bearer_identity().unwrap().clone(),
        ),
        PostgresActorIdentityResolver::new(pool.clone()),
    );
    crate::protected_application_router(pool.clone(), auth, MutationGuard::Workload)
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
async fn body(response: Response) -> Value {
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    serde_json::from_slice(&to_bytes(response.into_body(), 1_048_576).await.unwrap()).unwrap()
}
async fn send(
    app: &Router,
    method: &str,
    path: &str,
    payload: &Value,
    status: StatusCode,
) -> Value {
    let response = app
        .clone()
        .oneshot(request(method, path, payload))
        .await
        .unwrap();
    assert_eq!(response.status(), status, "{method} {path}");
    body(response).await
}
fn definitions() -> [(&'static str, Value); 3] {
    [
        (
            "asset-types",
            json!({"id":"equipment","version":1,"name":"Equipment","supported_properties":[]}),
        ),
        (
            "properties",
            json!({"id":"temperature","version":1,"name":"Temperature","value_kind":"decimal","canonical_unit":"celsius"}),
        ),
        (
            "relationship-types",
            json!({"id":"physical.contains","version":1,"name":"Contains"}),
        ),
    ]
}
async fn publish(app: &Router, resource: &str, payload: &Value) -> Value {
    send(
        app,
        "POST",
        &format!("{ROOT}/{resource}"),
        payload,
        StatusCode::CREATED,
    )
    .await
}
