//! Complete HTTP commissioning and explicit correction through the real router.
mod authorization;
mod commands;
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};
use tower::ServiceExt;
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
async fn setup(pool: &PgPool) -> Router {
    sqlx::raw_sql(include_str!("tests/topology.sql"))
        .execute(pool)
        .await
        .unwrap();
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
fn path(binding: bool, revision: &str) -> String {
    format!(
        "/api/v1/tenants/tenant-a/{}/fact-a/revisions/{revision}",
        if binding {
            "signal-bindings"
        } else {
            "relationships"
        }
    )
}
fn payload(binding: bool) -> Value {
    if binding {
        json!({"source":{"device_asset_id":"asset-a","endpoint_id":"can","signal_id":"bus.voltage"},
        "target":{"asset_id":"asset-b","property":{"id":"voltage","version":1}},"effective_interval":{"start_ms":0,"end_ms":100}})
    } else {
        json!({"relationship_type":{"id":"physical.contains","version":1},"source_asset_id":"asset-a","target_asset_id":"asset-b","effective_interval":{"start_ms":0,"end_ms":100}})
    }
}
fn request(method: &str, path: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap()
}
async fn send(app: &Router, method: &str, path: &str, body: &Value, status: StatusCode) -> Value {
    decode(
        app.clone()
            .oneshot(request(method, path, &body.to_string()))
            .await
            .unwrap(),
        status,
    )
    .await
}
async fn decode(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    serde_json::from_slice(&to_bytes(response.into_body(), 65_536).await.unwrap()).unwrap()
}
async fn count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT (SELECT count(*) FROM metadata.relationship_revision_records)+(SELECT count(*) FROM metadata.signal_binding_revision_records)").fetch_one(pool).await.unwrap()
}
