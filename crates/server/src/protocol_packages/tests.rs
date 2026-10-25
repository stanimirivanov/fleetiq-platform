use super::dto;
#[test]
fn digest_and_world_inputs_are_closed_and_canonical() {
    assert!(dto::digest(&"a0".repeat(32)).is_some());
    for text in [
        "A0".repeat(32),
        "a".repeat(63),
        "z".repeat(64),
        "é".repeat(32),
    ] {
        assert!(dto::digest(&text).is_none());
    }
    assert!(serde_json::from_str::<dto::AdmissionBody>(r#"{"world":"identity-v1"}"#).is_ok());
    assert!(serde_json::from_str::<dto::AdmissionBody>(r#"{"world":"other"}"#).is_err());
    assert!(
        serde_json::from_str::<dto::AdmissionBody>(r#"{"world":"runtime-v1","activate":true}"#)
            .is_err()
    );
}

use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use fleetiq_ingestion::{ProtocolAdmissionWorld, ProtocolArtifact, ProtocolCompilerError};
use serde_json::Value;
use tower::ServiceExt;
struct Compiler(Option<ProtocolCompilerError>);
impl ProtocolCompiler for Compiler {
    fn validate(
        &self,
        _: &ProtocolArtifact,
        _: ProtocolAdmissionWorld,
    ) -> impl Future<Output = Result<(), ProtocolCompilerError>> + Send {
        std::future::ready(self.0.map_or(Ok(()), Err))
    }
}
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROOT: &str = "/api/v1/tenants/tenant-a/protocol-packages/custom.binary/versions/1";
async fn setup(
    pool: &PgPool,
    error: Option<ProtocolCompilerError>,
    mutation: MutationGuard,
) -> Router {
    sqlx::raw_sql("INSERT INTO metadata.tenants(tenant_id) VALUES ('tenant-a'),('tenant-b'); INSERT INTO identity.actors(actor_id) VALUES ('operator-a'); INSERT INTO identity.external_actor_identities(authority_id,subject,actor_id) VALUES ('workload','operator','operator-a'); INSERT INTO identity.tenant_action_grants(tenant_id,actor_id,action) VALUES ('tenant-a','operator-a','protocol.manage'),('tenant-b','operator-a','metadata.manage');").execute(pool).await.unwrap();
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
    with_compiler(pool.clone(), auth, mutation, Compiler(error))
}
fn request(method: &str, path: &str, bytes: Vec<u8>, content_type: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", content_type)
        .header(
            "x-artifact-sha256",
            ProtocolArtifact::new(b"component-fixture".to_vec())
                .unwrap()
                .digest()
                .to_string(),
        )
        .body(Body::from(bytes))
        .unwrap()
}
async fn json_response(response: axum::response::Response) -> Value {
    assert_eq!(response.headers()["cache-control"], "no-store");
    serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap()
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn protected_upload_admission_and_exact_retries_preserve_identity(pool: PgPool) {
    let app = setup(&pool, None, MutationGuard::Workload).await;
    for status in [StatusCode::CREATED, StatusCode::OK] {
        let response = app
            .clone()
            .oneshot(request(
                "PUT",
                ROOT,
                b"component-fixture".to_vec(),
                "application/wasm",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        let body = json_response(response).await;
        assert_eq!(body["artifact_size_bytes"], 17);
    }
    let response = app
        .clone()
        .oneshot(request("GET", ROOT, vec![], "application/json"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let stored = json_response(response).await;
    crate::contract_test_support::assert_published_example(
        &serde_json::to_vec(&stored).unwrap(),
        "/api/v1/tenants/{tenant_id}/protocol-packages/{package_id}/versions/{package_version}",
    );
    let mut first = None;
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(request(
                "POST",
                &format!("{ROOT}/admissions"),
                br#"{"world":"identity-v1"}"#.to_vec(),
                "application/json",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_response(response).await;
        if let Some(first) = &first {
            assert_eq!(first, &body);
        } else {
            first = Some(body);
        }
    }
    let unauthorized = Request::builder().uri(ROOT).body(Body::empty()).unwrap();
    assert_eq!(
        app.clone().oneshot(unauthorized).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.clone()
            .oneshot(request(
                "GET",
                &ROOT.replace("tenant-a", "tenant-b"),
                vec![],
                "application/json"
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a' AND action='protocol.manage'").execute(&pool).await.unwrap();
    assert_eq!(
        app.oneshot(request("GET", ROOT, vec![], "application/json"))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn binary_digest_collision_oversize_and_worker_failures_are_not_success(pool: PgPool) {
    let app = setup(
        &pool,
        Some(ProtocolCompilerError::Rejected),
        MutationGuard::Workload,
    )
    .await;
    let response = app
        .clone()
        .oneshot(request(
            "PUT",
            ROOT,
            b"wrong-bytes".to_vec(),
            "application/wasm",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = app
        .clone()
        .oneshot(request(
            "PUT",
            ROOT,
            vec![0; MAX_PROTOCOL_ARTIFACT_BYTES + 1],
            "application/wasm",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        app.clone()
            .oneshot(request(
                "PUT",
                ROOT,
                b"component-fixture".to_vec(),
                "application/wasm"
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CREATED
    );
    let mut collision = request("PUT", ROOT, vec![9], "application/wasm");
    collision.headers_mut().insert(
        "x-artifact-sha256",
        ProtocolArtifact::new(vec![9])
            .unwrap()
            .digest()
            .to_string()
            .parse()
            .unwrap(),
    );
    assert_eq!(
        app.clone().oneshot(collision).await.unwrap().status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.oneshot(request(
            "POST",
            &format!("{ROOT}/admissions"),
            br#"{"world":"runtime-v1"}"#.to_vec(),
            "application/json"
        ))
        .await
        .unwrap()
        .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM ingestion.protocol_admissions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn browser_mutation_guard_precedes_any_upload_effect(pool: PgPool) {
    let app = setup(
        &pool,
        None,
        MutationGuard::Browser {
            public_origin: "https://fleet.example".to_owned(),
        },
    )
    .await;
    assert_eq!(
        app.clone()
            .oneshot(request(
                "PUT",
                ROOT,
                b"component-fixture".to_vec(),
                "application/wasm"
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let mut authorized = request(
        "PUT",
        ROOT,
        b"component-fixture".to_vec(),
        "application/wasm",
    );
    authorized
        .headers_mut()
        .insert("origin", "https://fleet.example".parse().unwrap());
    authorized
        .headers_mut()
        .insert("x-fleetiq-csrf", "1".parse().unwrap());
    assert_eq!(
        app.oneshot(authorized).await.unwrap().status(),
        StatusCode::CREATED
    );
}
#[test]
fn problem_statuses_are_explicit() {
    let rows = [
        (ProtocolCompilerError::Busy, StatusCode::TOO_MANY_REQUESTS),
        (
            ProtocolCompilerError::Timeout,
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            ProtocolCompilerError::Unavailable,
            StatusCode::SERVICE_UNAVAILABLE,
        ),
    ];
    for (error, status) in rows {
        assert_eq!(
            super::handlers::failure(&fleetiq_ingestion::ProtocolPackageServiceError::Compiler(
                error
            ))
            .status(),
            status
        );
    }
}
