//! Real migration-backed authority and JSON contract regressions.
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
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROOT: &str = "/api/v1/tenants/tenant-a/fleet?latitude_property=position.latitude&latitude_version=1&longitude_property=position.longitude&longitude_version=1&encoding=e7";
async fn setup(pool: &PgPool) -> Router {
    sqlx::raw_sql(include_str!(
        "../../../postgres/tests/property_history/topology.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!("../../../postgres/tests/fleet/topology.sql"))
        .execute(pool)
        .await
        .unwrap();
    PostgresCurrentStateRepository::new(pool.clone())
        .tick()
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
async fn response(app: &Router, url: &str, authenticated: bool) -> (StatusCode, Value) {
    let mut request = Request::get(url);
    if authenticated {
        request = request.header("authorization", format!("Bearer {TOKEN}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let status = response.status();
    let value =
        serde_json::from_slice(&to_bytes(response.into_body(), 2_097_152).await.unwrap()).unwrap();
    (status, value)
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn fleet_requires_both_capabilities_before_mapping_discovery(pool: PgPool) {
    let app = setup(&pool).await;
    assert_eq!(
        response(&app, ROOT, false).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(response(&app, ROOT, true).await.0, StatusCode::FORBIDDEN);
    sqlx::query("INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES ('tenant-a','operator-a','metadata.read')").execute(&pool).await.unwrap();
    assert_eq!(
        response(&app, &format!("{ROOT}&limit=0"), true).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        response(
            &app,
            &ROOT.replace("position.latitude", "missing.property"),
            true
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, body) = response(&app, ROOT, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["summary"]["assets"], "5");
    let located = body["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["asset_id"] == "located-a")
        .unwrap();
    assert_eq!(located["location"]["status"], "observed");
    assert_eq!(located["location"]["point"]["latitude"], "51.2345678");
    assert_eq!(located["location"]["point"]["longitude"], "-12.3456789");
    assert_eq!(located["location"]["point"]["quality"], "uncertain");
    let spec: Value =
        serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json")).unwrap();
    let schema = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","components":{"schemas":spec["components"]["schemas"]},"$ref":"#/components/schemas/FleetSnapshot"});
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&body));
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a' AND actor_id='operator-a' AND action='telemetry.read'").execute(&pool).await.unwrap();
    assert_eq!(response(&app, ROOT, true).await.0, StatusCode::FORBIDDEN);
}

struct SwitchingAuthentication {
    pool: PgPool,
    calls: std::sync::atomic::AtomicUsize,
}
impl RequestAuthenticator for SwitchingAuthentication {
    async fn authenticate(
        &self,
        headers: &HeaderMap,
    ) -> Result<fleetiq_access::AuthenticatedActor, ProtectedRequestError> {
        if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1 {
            sqlx::query("UPDATE identity.external_actor_identities SET actor_id='replacement' WHERE authority_id='workload' AND subject='operator'").execute(&self.pool).await.map_err(|_| ProtectedRequestError::Internal)?;
        }
        let config = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
            .unwrap()
            .with_bearer_identity(TOKEN, "workload", "operator")
            .unwrap();
        let delegate = CallerAuthenticationService::new(
            crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
                config.bearer_identity().unwrap().clone(),
            ),
            PostgresActorIdentityResolver::new(self.pool.clone()),
        );
        protected_request::authenticate(headers, &delegate).await
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn independent_capability_proofs_cannot_be_mixed_across_actor_reassignment(pool: PgPool) {
    let _app = setup(&pool).await;
    sqlx::raw_sql("INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES ('tenant-a','operator-a','metadata.read'); INSERT INTO identity.actors (actor_id) VALUES ('replacement'); INSERT INTO identity.tenant_action_grants (tenant_id,actor_id,action) VALUES ('tenant-a','replacement','telemetry.read');").execute(&pool).await.unwrap();
    let app = router(
        pool.clone(),
        SwitchingAuthentication {
            pool,
            calls: std::sync::atomic::AtomicUsize::new(0),
        },
    );
    assert_eq!(response(&app, ROOT, true).await.0, StatusCode::FORBIDDEN);
}
