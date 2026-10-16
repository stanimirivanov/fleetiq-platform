//! Composed routes, current grants and real persistence; no mock SQL outcomes.
mod authorization;
mod commands;
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Request, header},
};
use serde_json::{Value, json};
use tower::ServiceExt;
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROOT: &str = "/api/v1/tenants/tenant-a/assets";
async fn setup(pool: &PgPool) -> Router {
    sqlx::raw_sql("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a'),('tenant-b');
        INSERT INTO identity.actors (actor_id) VALUES ('operator-a');
        INSERT INTO identity.external_actor_identities (authority_id,subject,actor_id) VALUES ('workload','operator','operator-a');
        INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES
            ('tenant-a','operator-a','metadata.read'),('tenant-a','operator-a','metadata.manage');
        INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count)
            VALUES ('tenant-a','equipment',1,'Equipment',0),('tenant-b','equipment',1,'Equipment',0);")
        .execute(pool).await.unwrap();
    let config = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "operator")
        .unwrap();
    let auth = fleetiq_access::CallerAuthenticationService::new(
        crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
            config.bearer_identity().unwrap().clone(),
        ),
        fleetiq_postgres::PostgresActorIdentityResolver::new(pool.clone()),
    );
    crate::protected_application_router(pool.clone(), auth, MutationGuard::Workload)
}
fn asset() -> Value {
    json!({"id":"asset-a","name":"Machine","asset_type":{"id":"equipment","version":1},
        "external_identifiers":[{"kind":"imei","authority":null,"value":"350000000000001"}]})
}
fn device() -> Value {
    json!({"endpoints":[{"id":"can-primary","kind":"bus.can","name":"Primary CAN interface"}]})
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
    serde_json::from_slice(&to_bytes(response.into_body(), 65_536).await.unwrap()).unwrap()
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
async fn audit_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM metadata.registration_records")
        .fetch_one(pool)
        .await
        .unwrap()
}
