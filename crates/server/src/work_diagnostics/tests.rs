use super::*;
use crate::alerts::tests::{append_evidence, boundary, rule, setup};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;
const ROOT: &str = "/api/v1/tenants/tenant-a/work-diagnostics";
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
async fn read(app: &Router, path: &str, token: bool) -> (StatusCode, Value) {
    let mut request = Request::get(path);
    if token {
        request = request.header("authorization", format!("Bearer {TOKEN}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let status = response.status();
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    if status == StatusCode::OK {
        let spec: Value =
            serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json"))
                .unwrap();
        let schema =
            json!({"components":spec["components"],"$ref":"#/components/schemas/WorkDiagnostics"});
        assert!(
            jsonschema::validator_for(&schema).unwrap().is_valid(&body),
            "{body}"
        );
    }
    (status, body)
}
async fn grant(pool: &PgPool) {
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES('tenant-a','operator-a','operations.read')").execute(pool).await.unwrap();
}
fn queue<'a>(body: &'a Value, kind: &str) -> &'a Value {
    body["queues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["kind"] == kind)
        .unwrap()
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn authority_is_independent_current_and_precedes_discovery(pool: PgPool) {
    let app = setup(&pool).await;
    sqlx::query("INSERT INTO identity.tenant_action_grants SELECT 'tenant-a','operator-a',a FROM unnest(ARRAY['metadata.manage','access.manage','protocol.manage','analytics.manage','alerts.acknowledge']) a ON CONFLICT DO NOTHING").execute(&pool).await.unwrap();
    assert_eq!(read(&app, ROOT, false).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(read(&app, ROOT, true).await.0, StatusCode::FORBIDDEN);
    grant(&pool).await;
    let (status, body) = read(&app, ROOT, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["worker_liveness"], "not_observed");
    assert!(
        body["queues"]
            .as_array()
            .unwrap()
            .iter()
            .all(|q| q["sampled"] == 0)
    );
    assert_eq!(
        read(&app, "/api/v1/tenants/tenant-b/work-diagnostics", true)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='operations.read'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(read(&app, ROOT, true).await.0, StatusCode::FORBIDDEN);
    grant(&pool).await;
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(read(&app, ROOT, true).await.0, StatusCode::FORBIDDEN);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn lifecycle_and_delivery_samples_preserve_source_truth_and_closed_failures(pool: PgPool) {
    let app = setup(&pool).await;
    grant(&pool).await;
    rule(&pool, "high", "50", 0).await;
    let base = boundary(&pool).await;
    append_evidence(&pool, "e", base, Some("51")).await;
    let (_, before) = read(&app, ROOT, true).await;
    assert_eq!(queue(&before, "alert_evaluation")["eligible"], 1);
    assert!(
        fleetiq_postgres::PostgresAlertLifecycle::new(pool.clone())
            .advance_one()
            .await
            .unwrap()
    );
    let work = fleetiq_postgres::PostgresAlertDelivery::new(pool.clone());
    work.seed(&[7; 32]).await.unwrap();
    let lease = work.claim(&[7; 32]).await.unwrap().unwrap();
    let (_, leased) = read(&app, ROOT, true).await;
    assert_eq!(queue(&leased, "notification")["leased"], 1);
    work.finish(
        &lease,
        Err(fleetiq_analytics::AlertDeliveryFailure::Transport),
    )
    .await
    .unwrap();
    let (_, deferred) = read(&app, ROOT, true).await;
    assert_eq!(queue(&deferred, "notification")["deferred"], 1);
    assert_eq!(
        queue(&deferred, "notification")["failure_categories"],
        json!(["transport"])
    );
    sqlx::query("UPDATE analytics.alert_notification_delivery SET attempts=5,next_attempt_at=transaction_timestamp()-interval '1 day'").execute(&pool).await.unwrap();
    let (_, failed) = read(&app, ROOT, true).await;
    assert_eq!(queue(&failed, "notification")["failed"], 1);
    // Existing schedule fixture permits additional pending positions without rewriting done work.
    sqlx::query("INSERT INTO analytics.calculator_work(tenant_id,evaluation_id,schedule_id,start_ms,request) SELECT 'tenant-a','pending-'||g,'e',$1+g,'{}' FROM generate_series(1,65) g").bind(base).execute(&pool).await.unwrap();
    let (_, sample) = read(&app, ROOT, true).await;
    assert_eq!(queue(&sample, "calculator")["sampled"], 64);
    assert_eq!(queue(&sample, "calculator")["truncated"], true);
    assert_eq!(queue(&sample, "calculator")["blocked"], 64); // completed work is excluded
    sqlx::query("UPDATE analytics.calculator_work SET generation=3,attempts=3,lease_until_ms=0 WHERE evaluation_id='pending-1'").execute(&pool).await.unwrap();
    let (_, expired) = read(&app, ROOT, true).await;
    assert_eq!(queue(&expired, "calculator")["failed"], 1);
    let serialized = expired.to_string();
    for forbidden in [
        "target",
        "threshold",
        "destination",
        "postgres://",
        "definition",
    ] {
        assert!(!serialized.contains(forbidden), "{forbidden}");
    }
    let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM analytics.alert_notification_intents),(SELECT count(*) FROM analytics.alert_transitions)").fetch_one(&pool).await.unwrap();
    assert_eq!(counts, (1, 1));
}

#[sqlx::test(migrations = false)]
async fn upgrade_expands_authority_without_delegating_or_rewriting_grants(pool: PgPool) {
    for migration in fleetiq_postgres::MIGRATOR.iter().filter(|m| m.version < 50) {
        sqlx::raw_sql(migration.sql.clone())
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::raw_sql("INSERT INTO metadata.tenants(tenant_id) VALUES('tenant-a'); INSERT INTO identity.actors(actor_id) VALUES('operator-a'); INSERT INTO identity.tenant_action_grants VALUES('tenant-a','operator-a','metadata.read');").execute(&pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/0050_operational_read.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let before: Vec<String> =
        sqlx::query_scalar("SELECT action FROM identity.tenant_action_grants")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(before, vec!["metadata.read"]);
    grant(&pool).await;
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn blocked_storage_times_out_redacted_and_returns_pool_to_service(pool: PgPool) {
    let app = setup(&pool).await;
    grant(&pool).await;
    let mut lock = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE analytics.calculator_work IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock)
        .await
        .unwrap();
    let (status, body) = tokio::time::timeout(Duration::from_secs(8), read(&app, ROOT, true))
        .await
        .unwrap();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["title"], "Internal server error");
    for secret in ["statement_timeout", "calculator_work", "SELECT", "sqlx"] {
        assert!(!body.to_string().contains(secret));
    }
    lock.rollback().await.unwrap();
    assert_eq!(read(&app, ROOT, true).await.0, StatusCode::OK);
}
