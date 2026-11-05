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

async fn resolved_notification_history(pool: &PgPool) -> Router {
    let app = setup(pool).await;
    grant(pool).await;
    rule(pool, "high", "50", 0).await;
    let base = boundary(pool).await;
    let lifecycle = fleetiq_postgres::PostgresAlertLifecycle::new(pool.clone());
    append_evidence(pool, "firing", base, Some("51")).await;
    assert!(lifecycle.advance_one().await.unwrap());
    append_evidence(pool, "resolved", base + 1000, Some("0")).await;
    assert!(lifecycle.advance_one().await.unwrap());
    // Completed reconciliation histories sort before the live positions below.
    sqlx::query(
        "INSERT INTO analytics.alert_notification_delivery
         (destination,tenant_id,rule_id,revision,firing_version,target_version,accepted_version,resolved_until)
         SELECT decode(lpad(to_hex(g),64,'0'),'hex'),'tenant-a','high',1,1,2,2,
         transaction_timestamp()-interval '1 day' FROM generate_series(1,70) g",
    )
    .execute(pool)
    .await
    .unwrap();
    app
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn completed_history_does_not_hide_actionable_notification_states(pool: PgPool) {
    let app = resolved_notification_history(&pool).await;
    let (_, completed) = read(&app, ROOT, true).await;
    assert_eq!(queue(&completed, "notification")["sampled"], 0);
    assert_eq!(queue(&completed, "notification")["truncated"], false);

    sqlx::query(
        "INSERT INTO analytics.alert_notification_delivery
         (destination,tenant_id,rule_id,revision,firing_version,target_version,attempts,lease_until,next_attempt_at,last_failure)
         SELECT decode(repeat('ff',31)||lpad(to_hex(g),2,'0'),'hex'),'tenant-a','high',1,1,
         CASE WHEN g=5 THEN 1 ELSE 2 END,
         CASE WHEN g IN (1,5) THEN 5 ELSE 1 END,
         CASE WHEN g=3 THEN transaction_timestamp()+interval '1 day' ELSE NULL END,
         CASE WHEN g=4 THEN transaction_timestamp()+interval '1 day' ELSE transaction_timestamp()-interval '1 day' END,
         CASE WHEN g=1 THEN 'transport' ELSE NULL END
         FROM generate_series(1,5) g",
    )
    .execute(&pool)
    .await
    .unwrap();
    let before: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(d) ORDER BY destination) FROM analytics.alert_notification_delivery d",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let (status, body) = read(&app, ROOT, true).await;
    assert_eq!(status, StatusCode::OK);
    let notification = queue(&body, "notification");
    assert_eq!(notification["sampled"], 5);
    assert_eq!(notification["failed"], 1);
    assert_eq!(notification["eligible"], 2); // Includes the newer terminal successor.
    assert_eq!(notification["leased"], 1);
    assert_eq!(notification["deferred"], 1);
    assert_eq!(notification["truncated"], false);
    assert_eq!(notification["failure_categories"], json!(["transport"]));
    assert!(notification["oldest_sampled_eligible_at_ms"].is_string());
    let (_, repeated) = read(&app, ROOT, true).await;
    assert_eq!(queue(&repeated, "notification"), notification);

    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES('tenant-a','operator-a','operations.manage')")
        .execute(&pool).await.unwrap();
    let paused = crate::work_recovery::tests::send(
        &app,
        "notification",
        "pause-diagnostics",
        json!({"operation":"pause","expected_generation":"0","target":{}}),
    )
    .await;
    assert_eq!(paused.0, StatusCode::OK);
    let (_, paused) = read(&app, ROOT, true).await;
    assert_eq!(queue(&paused, "notification")["blocked"], 3);
    assert_eq!(queue(&paused, "notification")["failed"], 1);
    assert_eq!(queue(&paused, "notification")["leased"], 1);
    let after: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(d) ORDER BY destination) FROM analytics.alert_notification_delivery d",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(before, after);
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn notification_sample_prioritizes_failures_and_due_work_before_backoff(pool: PgPool) {
    let app = resolved_notification_history(&pool).await;
    sqlx::query(
        "INSERT INTO analytics.alert_notification_delivery
         (destination,tenant_id,rule_id,revision,firing_version,target_version,attempts,next_attempt_at,last_failure)
         SELECT decode(lpad(to_hex(g),64,'f'),'hex'),'tenant-a','high',1,1,2,
         CASE WHEN g=72 THEN 5 ELSE 1 END,
         CASE WHEN g<=70 THEN transaction_timestamp()+interval '1 day' ELSE transaction_timestamp()-interval '1 day' END,
         CASE WHEN g=72 THEN 'rejected' ELSE NULL END
         FROM generate_series(1,72) g",
    )
    .execute(&pool)
    .await
    .unwrap();
    let (status, body) = read(&app, ROOT, true).await;
    assert_eq!(status, StatusCode::OK);
    let notification = queue(&body, "notification");
    assert_eq!(notification["sampled"], 64);
    assert_eq!(notification["truncated"], true);
    assert_eq!(notification["failed"], 1);
    assert_eq!(notification["eligible"], 1);
    assert_eq!(notification["deferred"], 62);
    assert_eq!(notification["failure_categories"], json!(["rejected"]));
    // Exactly 64 remaining candidates must not mistake completed history for a sentinel.
    sqlx::query(
        "UPDATE analytics.alert_notification_delivery SET accepted_version=2,
         resolved_until=transaction_timestamp()-interval '1 day'
         WHERE destination IN (
           SELECT decode(lpad(to_hex(g),64,'f'),'hex') FROM generate_series(1,8) g)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let (_, exact) = read(&app, ROOT, true).await;
    let notification = queue(&exact, "notification");
    assert_eq!(notification["sampled"], 64);
    assert_eq!(notification["truncated"], false);
    assert_eq!(notification["failed"], 1);
    assert_eq!(notification["eligible"], 1);
}
