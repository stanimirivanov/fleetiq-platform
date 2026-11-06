use crate::alerts::tests::setup;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;
async fn read(app: &Router, tenant: &str, token: bool) -> (StatusCode, Value) {
    let mut request = Request::get(format!("/api/v1/tenants/{tenant}/storage-usage"));
    if token {
        request = request.header(
            "authorization",
            "Bearer aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let status = response.status();
    let body =
        serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
    (status, body)
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn storage_usage_reconciles_rows_and_requires_current_independent_authority(pool: PgPool) {
    let app = setup(&pool).await;
    assert_eq!(
        read(&app, "tenant-a", false).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(read(&app, "tenant-a", true).await.0, StatusCode::FORBIDDEN);
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES ('tenant-a','operator-a','operations.read')").execute(&pool).await.unwrap();
    let (status, body) = read(&app, "tenant-a", true).await;
    assert_eq!(status, StatusCode::OK);
    let spec: Value =
        serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json")).unwrap();
    assert_eq!(
        spec["paths"]["/api/v1/tenants/{tenant_id}/storage-usage"]["get"]["security"],
        serde_json::json!([{"bearerAuth":[]},{"cookieAuth":[]}])
    );
    let schema = serde_json::json!({"components":spec["components"],"$ref":"#/components/schemas/StorageUsage"});
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&body));
    assert_eq!(body["physical_allocation"], "not_measured");
    assert_eq!(body["archive"], "not_measured");
    let payloads: Vec<Vec<u8>> = sqlx::query_scalar(
        "SELECT payload FROM ingestion.accepted_messages WHERE tenant_id='tenant-a'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(body["classes"][0]["records"], payloads.len().to_string());
    assert_eq!(
        body["classes"][0]["bytes"],
        payloads.iter().map(Vec::len).sum::<usize>().to_string()
    );
    for (index, sql) in [
        (
            1,
            "SELECT row_to_json(r)::text FROM ingestion.decoded_observations r WHERE tenant_id='tenant-a'",
        ),
        (
            2,
            "SELECT row_to_json(r)::text FROM history.asset_property_changes r WHERE tenant_id='tenant-a'",
        ),
    ] {
        let rows: Vec<String> = sqlx::query_scalar(sql).fetch_all(&pool).await.unwrap();
        assert_eq!(body["classes"][index]["records"], rows.len().to_string());
        assert_eq!(
            body["classes"][index]["bytes"],
            rows.iter().map(String::len).sum::<usize>().to_string()
        );
    }
    assert_eq!(read(&app, "tenant-b", true).await.0, StatusCode::FORBIDDEN);
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES ('tenant-b','operator-a','operations.read')").execute(&pool).await.unwrap();
    let (status, empty) = read(&app, "tenant-b", true).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        empty["classes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["records"] == "0" && m["bytes"] == "0")
    );
    let mut lock = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE ingestion.decoded_observations IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock)
        .await
        .unwrap();
    assert_eq!(
        read(&app, "tenant-a", true).await.0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    lock.rollback().await.unwrap();
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a' AND action='operations.read'").execute(&pool).await.unwrap();
    assert_eq!(read(&app, "tenant-a", true).await.0, StatusCode::FORBIDDEN);
}
