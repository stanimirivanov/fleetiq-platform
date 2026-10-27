//! Real HTTP/PostgreSQL authority, no-store, exact values and contract evidence.
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use fleetiq_access::CallerAuthenticationService;
use fleetiq_postgres::PostgresActorIdentityResolver;
use fleetiq_projection::ProjectionMaintenance;
use serde_json::{Value, json};
use tower::ServiceExt;
const ROOT: &str =
    "/api/v1/tenants/tenant-a/assets/asset-a/properties/electrical.voltage/versions/1/current";
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
async fn setup(pool: &PgPool) -> Router {
    sqlx::raw_sql(include_str!(
        "../../../postgres/tests/property_history/topology.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
    let config = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "operator")
        .unwrap();
    router(
        pool.clone(),
        CallerAuthenticationService::new(
            crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
                config.bearer_identity().unwrap().clone(),
            ),
            PostgresActorIdentityResolver::new(pool.clone()),
        ),
    )
}
async fn response(app: &Router, path: &str, authenticated: bool) -> (StatusCode, Value) {
    let mut request = Request::get(path);
    if authenticated {
        request = request.header("authorization", format!("Bearer {TOKEN}"));
    }
    let res = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.headers()["cache-control"], "no-store");
    let status = res.status();
    (
        status,
        serde_json::from_slice(&to_bytes(res.into_body(), 262_144).await.unwrap()).unwrap(),
    )
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn snapshot_contract_distinguishes_pending_missing_stale_and_exact_values(pool: PgPool) {
    let app = setup(&pool).await;
    let (status, mut before) = response(&app, ROOT, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(before["items"], json!([]));
    assert_eq!(before["pending"], true);
    let spec: Value =
        serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json")).unwrap();
    let schema = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","components":{"schemas":spec["components"]["schemas"]},"$ref":"#/components/schemas/CurrentPropertySnapshot"});
    assert!(
        jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(&before)
    );
    // Evaluation is server time; normalize only this nondeterministic field for
    // equality with the published neutral pre-projection example.
    before["evaluated_at_ms"] = json!("2000");
    assert_eq!(
        before,
        spec["components"]["responses"]["CurrentPropertySucceeded"]["content"]["application/json"]
            ["example"]
    );
    PostgresCurrentStateRepository::new(pool.clone())
        .tick()
        .await
        .unwrap();
    let (status, body) = response(&app, ROOT, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["revision"], "1");
    assert_eq!(body["pending"], false);
    assert_eq!(body["items"][0]["freshness"], "stale");
    assert_eq!(
        body["items"][0]["change"]["value"]["value"],
        "9223372036854775807"
    );
    assert_eq!(body["items"][0]["change"]["quality"], "good");
    assert!(body["items"][0]["change"]["interpretation"].is_string());
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&body));
    assert_eq!(
        response(&app, &ROOT.replace("versions/1", "versions/2"), true)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let mut invalid = body;
    let duplicate = invalid["items"][0].clone();
    invalid["items"].as_array_mut().unwrap().push(duplicate);
    assert!(
        !jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(&invalid)
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn current_authority_revocation_suspension_and_storage_failure_are_safe(pool: PgPool) {
    let app = setup(&pool).await;
    assert_eq!(
        response(&app, ROOT, false).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        response(&app, &ROOT.replace("tenant-a", "tenant-b"), true)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        response(&app, &ROOT.replace("versions/1", "versions/zero"), true)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    sqlx::query("ALTER TABLE projection.tenant_heads RENAME TO unavailable_heads")
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = response(&app, ROOT, true).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!body.to_string().contains("unavailable_heads"));
    sqlx::query("ALTER TABLE projection.unavailable_heads RENAME TO tenant_heads")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='telemetry.read'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        response(&app, &ROOT.replace("versions/1", "versions/zero"), true)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES ('tenant-a','operator-a','telemetry.read')").execute(&pool).await.unwrap();
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(response(&app, ROOT, true).await.0, StatusCode::FORBIDDEN);
}
