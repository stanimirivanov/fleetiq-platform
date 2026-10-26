use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use fleetiq_domain::{ActorId, TenantId, UnixTimeMillis};
use fleetiq_ingestion::{
    PROTOCOL_ADMISSION_POLICY, ProtocolAdmission, ProtocolAdmissionWorld, ProtocolArtifact,
    ProtocolArtifactStore, ProtocolPackageCatalog, ProtocolPackageRelease,
};
use fleetiq_postgres::{PostgresProtocolArtifactStore, PostgresProtocolPackageCatalog};
use fleetiq_telemetry::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};
use serde_json::{Value, json};
use tower::ServiceExt;
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROOT: &str = "/api/v1/tenants/tenant-a/assets/device-a/ingestion-diagnostics";
async fn setup(pool: &PgPool) -> Router {
    sqlx::raw_sql("INSERT INTO metadata.tenants(tenant_id) VALUES ('tenant-a'),('tenant-b');
        INSERT INTO identity.actors(actor_id) VALUES ('operator-a');
        INSERT INTO identity.external_actor_identities(authority_id,subject,actor_id) VALUES ('workload','operator','operator-a');
        INSERT INTO identity.tenant_action_grants(tenant_id,actor_id,action) VALUES ('tenant-a','operator-a','protocol.manage'),('tenant-b','operator-a','metadata.manage');
        INSERT INTO metadata.asset_type_definitions(tenant_id,type_id,version,display_name,property_count) VALUES ('tenant-a','equipment',1,'Equipment',0);
        INSERT INTO metadata.assets(tenant_id,asset_id,type_id,type_version,display_name) VALUES ('tenant-a','device-a','equipment',1,'Device');
        INSERT INTO metadata.communication_devices(tenant_id,device_asset_id) VALUES ('tenant-a','device-a');").execute(pool).await.unwrap();
    let artifact = ProtocolArtifact::new(b"fixture".to_vec()).unwrap();
    let package = ProtocolPackageRef::new(
        ProtocolPackageId::new("custom.binary").unwrap(),
        ProtocolPackageVersion::new("1").unwrap(),
    );
    let release = ProtocolPackageRelease::new(
        TenantId::new("tenant-a").unwrap(),
        package,
        artifact.digest(),
        artifact.size_bytes(),
    );
    PostgresProtocolPackageCatalog::new(pool.clone())
        .register(&release)
        .await
        .unwrap();
    let store = PostgresProtocolArtifactStore::new(pool.clone());
    store.put(&release, &artifact).await.unwrap();
    store
        .record(&ProtocolAdmission {
            release,
            world: ProtocolAdmissionWorld::Identity,
            policy_version: PROTOCOL_ADMISSION_POLICY,
            actor_id: ActorId::new("operator-a").unwrap(),
            admitted_at: UnixTimeMillis::new(1),
        })
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
    router(pool.clone(), auth)
}

fn request(path: &str, token: bool) -> Request<Body> {
    let mut r = Request::get(path);
    if token {
        r = r.header("authorization", format!("Bearer {TOKEN}"));
    }
    r.body(Body::empty()).unwrap()
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn diagnostics_authorize_before_lookup_and_return_safe_empty_activity(pool: PgPool) {
    let router = setup(&pool).await;
    for (path, token, status) in [
        (ROOT, false, StatusCode::UNAUTHORIZED),
        (
            "/api/v1/tenants/tenant-b/assets/device-a/ingestion-diagnostics",
            true,
            StatusCode::FORBIDDEN,
        ),
        (
            "/api/v1/tenants/tenant-a/assets/missing/ingestion-diagnostics",
            true,
            StatusCode::NOT_FOUND,
        ),
        (ROOT, true, StatusCode::OK),
    ] {
        let response = router.clone().oneshot(request(path, token)).await.unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        if status == StatusCode::OK {
            let spec: Value =
                serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json"))
                    .unwrap();
            let example = &spec["components"]["responses"]["IngestionDiagnosticsSucceeded"]["content"]
                ["application/json"]["example"];
            let mut actual = body.clone();
            actual["observed_at_ms"] = example["observed_at_ms"].clone();
            assert_eq!(&actual, example);
            assert_eq!(body["worker_liveness"], "not_observed");
            assert_eq!(body["connection_liveness"], "not_observed");
            assert_eq!(body["frames"], json!([]));
            assert_eq!(body["sources"], json!([]));
            assert_eq!(body["coverage"]["frames_truncated"], false);
        }
    }
    // Identity mapping and policy are rechecked; previously obtained responses
    // grant no authority to a subsequent request after revocation.
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='protocol.manage'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        router.oneshot(request(ROOT, true)).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn populated_diagnostics_conform_and_exclude_private_wire_and_values(pool: PgPool) {
    let router = setup(&pool).await;
    sqlx::raw_sql("INSERT INTO ingestion.accepted_messages(tenant_id,message_id,device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload) VALUES('tenant-a','message-safe','device-a','custom.binary','1',100,'private-wire-sentinel'::bytea);
        INSERT INTO ingestion.decoded_message_results(tenant_id,message_id,protocol_package_id,protocol_package_version,observation_count) VALUES('tenant-a','message-safe','custom.binary','1',1);
        INSERT INTO ingestion.decoded_observations(tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,endpoint_id,signal_id,value_kind,value_text,quality) VALUES('tenant-a','message-safe','custom.binary','1',0,'main','temperature','text','private-value-sentinel','good');").execute(&pool).await.unwrap();
    let response = router.clone().oneshot(request(ROOT, true)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(!text.contains("private-wire-sentinel") && !text.contains("private-value-sentinel"));
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let contract: Value =
        serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json")).unwrap();
    let schema = json!({"$schema":"https://json-schema.org/draft/2020-12/schema",
        "components":{"schemas":contract["components"]["schemas"]},"$ref":"#/components/schemas/IngestionDiagnostics"});
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&body));
    assert_eq!(body["frames"][0]["processing"], "decoded");
    assert_eq!(body["sources"][0]["signal_id"], "temperature");
    assert_eq!(body["coverage"]["observations_truncated"], false);
    // A database failure is not an empty healthy response or an SQL diagnostic.
    sqlx::query("ALTER TABLE ingestion.decoded_observations RENAME TO unavailable_fixture")
        .execute(&pool)
        .await
        .unwrap();
    let response = router.oneshot(request(ROOT, true)).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["title"], "Internal server error");
    assert!(
        !std::str::from_utf8(&bytes)
            .unwrap()
            .contains("decoded_observations")
    );
}
