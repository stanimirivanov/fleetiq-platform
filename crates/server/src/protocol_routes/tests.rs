use super::dto::ChangeBody;
#[test]
fn route_commands_require_explicit_retirement_and_reject_forged_context() {
    for body in [
        r#"{"expected_revision":0,"selection":null}"#,
        r#"{"expected_revision":1,"selection":{"package_id":"custom","package_version":"1","world":"identity-v1"}}"#,
    ] {
        assert!(
            serde_json::from_str::<ChangeBody>(body)
                .unwrap()
                .selection()
                .is_ok()
        );
    }
    for body in [
        r#"{"expected_revision":0}"#,
        r#"{"expected_revision":-1,"selection":null}"#,
        r#"{"expected_revision":0,"selection":null,"actor_id":"forged"}"#,
        r#"{"expected_revision":0,"selection":{"package_id":"p","package_version":"1","world":"unknown"}}"#,
    ] {
        assert!(serde_json::from_str::<ChangeBody>(body).is_err(), "{body}");
    }
}

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
const ROOT: &str = "/api/v1/tenants/tenant-a/assets/device-a/protocol-routes/tcp-a";
async fn setup(pool: &PgPool, mutation: MutationGuard) -> Router {
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
    router(pool.clone(), auth, mutation)
}
fn command(expected: u32, retire: bool) -> Value {
    json!({"expected_revision":expected,"selection":if retire{Value::Null}else{json!({"package_id":"custom.binary","package_version":"1","world":"identity-v1"})}})
}
fn request(method: &str, path: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}
async fn read(response: axum::response::Response) -> Value {
    assert_eq!(response.headers()["cache-control"], "no-store");
    serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap()
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn protected_route_commands_preserve_receipts_and_expose_retirement(pool: PgPool) {
    let app = setup(&pool, MutationGuard::Workload).await;
    let response = app
        .clone()
        .oneshot(request("PUT", ROOT, &command(0, false)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let initial = read(response).await;
    let replay = app
        .clone()
        .oneshot(request("PUT", ROOT, &command(0, false)))
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(read(replay).await, initial);
    assert_eq!(
        app.clone()
            .oneshot(request("GET", ROOT, &Value::Null))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.clone()
            .oneshot(request("PUT", ROOT, &command(0, true)))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let retired = app
        .clone()
        .oneshot(request("PUT", ROOT, &command(1, true)))
        .await
        .unwrap();
    assert_eq!(retired.status(), StatusCode::OK);
    let retired = read(retired).await;
    assert_eq!(retired["revision"], 2);
    assert_eq!(retired["selection"], Value::Null);
    assert_eq!(retired["policy_version"], Value::Null);
    let historical = app
        .clone()
        .oneshot(request("PUT", ROOT, &command(0, false)))
        .await
        .unwrap();
    assert_eq!(historical.status(), StatusCode::OK);
    assert_eq!(read(historical).await, initial);
    assert_eq!(
        read(
            app.clone()
                .oneshot(request("GET", ROOT, &Value::Null))
                .await
                .unwrap()
        )
        .await,
        retired
    );
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    for method in ["GET", "PUT"] {
        assert_eq!(
            app.clone()
                .oneshot(request(method, ROOT, &command(2, false)))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn route_denials_precede_body_diagnostics_and_never_configure_a_device(pool: PgPool) {
    let app = setup(&pool, MutationGuard::Workload).await;
    let mut anonymous = request("PUT", ROOT, &json!({"forged":true}));
    anonymous.headers_mut().remove("authorization");
    assert_eq!(
        app.clone().oneshot(anonymous).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.clone()
            .oneshot(request(
                "PUT",
                &ROOT.replace("tenant-a", "tenant-b"),
                &json!({"forged":true})
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    for body in [
        json!({"expected_revision":0}),
        json!({"expected_revision":4_294_967_296_u64,"selection":null}),
        json!({"expected_revision":0,"selection":null,"actor_id":"forged"}),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request("PUT", ROOT, &body))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let mut invalid = command(0, false);
    invalid["selection"]["world"] = json!("runtime-v1");
    assert_eq!(
        app.clone()
            .oneshot(request("PUT", ROOT, &invalid))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.clone()
            .oneshot(request(
                "PUT",
                &ROOT.replace("device-a", "missing"),
                &command(0, false)
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.clone()
            .oneshot(request("GET", ROOT, &Value::Null))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        app.oneshot(request("PUT", ROOT, &command(0, false)))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM ingestion.protocol_route_heads")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn browser_route_changes_require_exact_origin_and_csrf_marker(pool: PgPool) {
    let app = setup(
        &pool,
        MutationGuard::Browser {
            public_origin: "https://fleet.example".into(),
        },
    )
    .await;
    assert_eq!(
        app.clone()
            .oneshot(request("PUT", ROOT, &command(0, false)))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let mut valid = request("PUT", ROOT, &command(0, false));
    valid
        .headers_mut()
        .insert("origin", "https://fleet.example".parse().unwrap());
    valid
        .headers_mut()
        .insert("x-fleetiq-csrf", "1".parse().unwrap());
    assert_eq!(app.oneshot(valid).await.unwrap().status(), StatusCode::OK);
}
