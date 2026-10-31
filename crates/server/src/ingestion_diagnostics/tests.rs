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
    crate::protected_application_router(
        pool.clone(),
        auth,
        crate::mutation_guard::MutationGuard::Workload,
    )
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

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn operational_replay_sample_distinguishes_lease_expiry_and_poison(pool: PgPool) {
    use fleetiq_ingestion::ReplayRepository;
    let app = setup(&pool).await;
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES('tenant-a','operator-a','operations.read')").execute(&pool).await.unwrap();
    let work = replay_work();
    let repo = fleetiq_postgres::PostgresReplayRepository::new(pool.clone());
    repo.enqueue(&work).await.unwrap();
    for (state, sql) in [
        ("eligible", None),
        (
            "leased",
            Some(
                "UPDATE ingestion.replay_work SET generation=1,attempts=1,lease_until_ms=9223372036854775807",
            ),
        ),
        (
            "failed",
            Some(
                "UPDATE ingestion.replay_work SET generation=1,attempts=1,lease_until_ms=0,status='poisoned'",
            ),
        ),
    ] {
        if let Some(sql) = sql {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        let response = app
            .clone()
            .oneshot(request("/api/v1/tenants/tenant-a/work-diagnostics", true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let replay = value["queues"]
            .as_array()
            .unwrap()
            .iter()
            .find(|q| q["kind"] == "replay")
            .unwrap();
        assert_eq!(replay[state], 1);
        if state == "failed" {
            assert_eq!(replay["failure_categories"], json!([]));
        }
        if state == "eligible" {
            assert_eq!(replay["oldest_sampled_eligible_at_ms"], "100");
        }
        assert!(!String::from_utf8_lossy(&bytes).contains("private-fixture-payload"));
    }
}

fn replay_work() -> fleetiq_ingestion::ReplayWork {
    use fleetiq_ingestion::ReplayWork;
    use fleetiq_telemetry::{AcceptedMessage, AcceptedMessageId, AcceptedPayload};
    let artifact = ProtocolArtifact::new(b"fixture".to_vec()).unwrap();
    ReplayWork {
        message: AcceptedMessage::new(
            AcceptedMessageId::new("m").unwrap(),
            TenantId::new("tenant-a").unwrap(),
            fleetiq_domain::AssetId::new("device-a").unwrap(),
            ProtocolPackageRef::new(
                ProtocolPackageId::new("custom.binary").unwrap(),
                ProtocolPackageVersion::new("1").unwrap(),
            ),
            UnixTimeMillis::new(100),
            AcceptedPayload::new(b"private-fixture-payload".as_slice()),
        ),
        session: "s".into(),
        sequence: 1,
        digest: artifact.digest(),
        world: ProtocolAdmissionWorld::Identity,
        execution: fleetiq_protocol_sdk::replay_wire::replay_fingerprint(),
        pre_state: vec![],
    }
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn audited_replay_recovery_preserves_pre_state_and_fences_old_effects(pool: PgPool) {
    use crate::work_recovery::tests::{command, grant, send, view};
    use fleetiq_ingestion::{ReplayEffects, ReplayRepository};
    let app = setup(&pool).await;
    grant(&pool).await;
    let work = replay_work();
    let repo = fleetiq_postgres::PostgresReplayRepository::new(pool.clone());
    repo.enqueue(&work).await.unwrap();
    let old = repo.claim().await.unwrap().unwrap();
    assert_eq!(
        send(
            &app,
            "replay",
            "active",
            command("retry", old.generation, json!({"id":"m"}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    // Controlled crash/expiry fixture: terminal work keeps its recorded lease.
    sqlx::query("UPDATE ingestion.replay_work SET generation=3,attempts=3,status='poisoned',lease_until_ms=0")
        .execute(&pool)
        .await
        .unwrap();
    let unsupported=sqlx::query("SELECT analytics.recover_work('tenant-a','operator-a','unsupported','replay','retry','{\"id\":\"m\"}',3,NULL)").execute(&pool).await.unwrap_err();
    assert_eq!(
        unsupported.as_database_error().unwrap().code().as_deref(),
        Some("23505")
    );
    let (_, candidates) = view(&app, "replay").await;
    assert_eq!(candidates["targets"][0]["expected_generation"], "3");
    let retry = command("retry", 3, json!({"id":"m"}));
    let receipt = send(&app, "replay", "retry-a", retry.clone()).await;
    assert_eq!(receipt.0, StatusCode::OK);
    assert_eq!(
        send(&app, "replay", "pause-a", command("pause", 0, json!({})))
            .await
            .0,
        StatusCode::OK
    );
    assert!(repo.claim().await.unwrap().is_none());
    assert_eq!(
        send(&app, "replay", "resume-a", command("resume", 1, json!({})))
            .await
            .0,
        StatusCode::OK
    );
    let current = repo.claim().await.unwrap().unwrap();
    assert_eq!(current.generation, 5);
    assert_eq!(current.work, old.work);
    let (batch, state) = ReplayEffects {
        outputs: vec![],
        state: vec![1],
    }
    .batch(&work)
    .unwrap();
    assert!(!repo.complete(&old, &batch, &state).await.unwrap());
    assert!(!repo.poison(&old).await.unwrap());
    assert!(repo.complete(&current, &batch, &state).await.unwrap());
    assert_eq!(send(&app, "replay", "retry-a", retry).await, receipt);
    assert_eq!(
        send(
            &app,
            "replay",
            "completed",
            command("retry", 5, json!({"id":"m"}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}
#[sqlx::test(migrations = false)]
async fn recovery_upgrade_retains_old_terminal_work_and_attempt_budget(pool: PgPool) {
    use fleetiq_ingestion::ReplayRepository;
    for migration in fleetiq_postgres::MIGRATOR.iter().filter(|m| m.version < 51) {
        sqlx::raw_sql(migration.sql.clone())
            .execute(&pool)
            .await
            .unwrap();
    }
    let _app = setup(&pool).await;
    let work = replay_work();
    let repo = fleetiq_postgres::PostgresReplayRepository::new(pool.clone());
    repo.enqueue(&work).await.unwrap();
    sqlx::query("UPDATE ingestion.replay_work SET generation=2,lease_until_ms=0,status='poisoned'")
        .execute(&pool)
        .await
        .unwrap();
    let before: Value = sqlx::query_scalar("SELECT to_jsonb(w) FROM ingestion.replay_work w")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/0051_audited_work_recovery.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let (after, attempts): (Value, i32) =
        sqlx::query_as("SELECT to_jsonb(w)-'attempts',attempts FROM ingestion.replay_work w")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(before, after);
    assert_eq!(attempts, 2);
    assert!(sqlx::query("UPDATE ingestion.replay_work SET generation=3,attempts=0,lease_until_ms=NULL,status='pending'").execute(&pool).await.is_err());
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM identity.tenant_action_grants WHERE action='operations.manage'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
}
