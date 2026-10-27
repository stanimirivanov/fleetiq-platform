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
const ROOT: &str = "/api/v1/tenants/tenant-a/assets/asset-a/semantic-graph";
async fn setup(pool: &PgPool) -> Router {
    sqlx::raw_sql(include_str!(
        "../../../postgres/tests/semantic_graph/topology.sql"
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
fn request(path: &str, authenticated: bool) -> Request<Body> {
    let mut request = Request::get(path);
    if authenticated {
        request = request.header("authorization", format!("Bearer {TOKEN}"));
    }
    request.body(Body::empty()).unwrap()
}
async fn response(app: &Router, path: &str) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request(path, true)).await.unwrap();
    let status = response.status();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let bytes = to_bytes(response.into_body(), 262_144).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn graph_matches_published_example_and_preserves_directed_semantic_evidence(pool: PgPool) {
    let app = setup(&pool).await;
    let url = format!("{ROOT}?effective_at_ms=50&known_at_ms=50");
    let (status, body) = response(&app, &url).await;
    assert_eq!(status, StatusCode::OK);
    let spec: Value =
        serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json")).unwrap();
    assert_eq!(
        body,
        spec["components"]["responses"]["SemanticGraphSucceeded"]["content"]["application/json"]["example"]
    );
    let schema = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","components":{"schemas":spec["components"]["schemas"]},"$ref":"#/components/schemas/SemanticGraph"});
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&body));
    assert_eq!(body["relationships"][2]["source_asset_id"], "asset-c");
    assert_eq!(
        body["bindings"][0]["source"]["device_asset_id"],
        "gateway-1"
    );
    let (status, cut) = response(&app, &format!("{url}&depth=0")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cut["assets"].as_array().unwrap().len(), 1);
    assert_eq!(cut["relationships"].as_array().unwrap().len(), 1);
    assert_eq!(cut["truncated"], true);
    assert_eq!(cut["truncation"], json!(["depth"]));
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&cut));
    let mut invalid = cut;
    invalid["truncated"] = json!(false);
    assert!(
        !jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(&invalid)
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn current_metadata_authority_precedes_query_validation_and_tenant_discovery(pool: PgPool) {
    let app = setup(&pool).await;
    assert_eq!(
        app.clone()
            .oneshot(request(ROOT, false))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        response(&app, &ROOT.replace("tenant-a", "tenant-b"))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let url = format!("{ROOT}?effective_at_ms=50&known_at_ms=50");
    for suffix in [
        "&depth=5",
        "&depth=256",
        "&asset_limit=0",
        "&asset_limit=101",
        "&relationship_limit=0",
        "&relationship_limit=201",
        "&binding_limit=0",
        "&binding_limit=201",
        "&unknown=true",
    ] {
        assert_eq!(
            response(&app, &format!("{url}{suffix}")).await.0,
            StatusCode::BAD_REQUEST,
            "{suffix}"
        );
    }
    assert_eq!(response(&app, ROOT).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        response(&app, &url.replace("assets/asset-a", "assets/missing"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a' AND action='metadata.read'").execute(&pool).await.unwrap();
    assert_eq!(response(&app, &url).await.0, StatusCode::FORBIDDEN);
    assert_eq!(response(&app, ROOT).await.0, StatusCode::FORBIDDEN);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn tenant_suspension_and_storage_failure_never_become_empty_complete_graphs(pool: PgPool) {
    let app = setup(&pool).await;
    let url = format!("{ROOT}?effective_at_ms=50&known_at_ms=50");
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(response(&app, &url).await.0, StatusCode::FORBIDDEN);
    sqlx::query("UPDATE metadata.tenants SET is_active=true WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "ALTER TABLE metadata.asset_relationship_revisions RENAME TO unavailable_graph_fixture",
    )
    .execute(&pool)
    .await
    .unwrap();
    let (status, body) = response(&app, &url).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["title"], "Internal server error");
    assert!(!body.to_string().contains("asset_relationship_revisions"));
}
