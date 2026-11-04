use super::*;
use crate::alerts::tests::{append_evidence, boundary, rule, setup};
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use fleetiq_analytics::{CalculatorPreparation, calculate_prepared};
use fleetiq_postgres::{
    PostgresAlertDelivery, PostgresAlertLifecycle, PostgresCalculatorPreparation,
    PostgresCalculatorWork,
};
use serde_json::Value;
use tower::ServiceExt;
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(crate) async fn request(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    token: bool,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if token {
        req = req.header("authorization", format!("Bearer {TOKEN}"));
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    assert_eq!(res.headers()["cache-control"], "no-store");
    let status = res.status();
    let body: Value =
        serde_json::from_slice(&to_bytes(res.into_body(), 32768).await.unwrap()).unwrap();
    if !path.contains("/work-recovery/") && status == StatusCode::OK {
        return (status, body);
    }
    let spec: Value =
        serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json")).unwrap();
    let name = if status == StatusCode::OK {
        if method == "GET" {
            "WorkRecoveryTargets"
        } else {
            "WorkRecoveryReceipt"
        }
    } else {
        "Problem"
    };
    let schema =
        json!({"components":spec["components"],"$ref":format!("#/components/schemas/{name}")});
    assert!(
        jsonschema::validator_for(&schema).unwrap().is_valid(&body),
        "{body}"
    );
    (status, body)
}
pub(crate) fn command(operation: &str, g: i64, target: Value) -> Value {
    let mut value = json!({"operation":operation,"expected_generation":g.to_string()});
    value
        .as_object_mut()
        .expect("command object")
        .insert("target".into(), target);
    value
}
pub(crate) async fn send(app: &Router, kind: &str, id: &str, body: Value) -> (StatusCode, Value) {
    request(
        app,
        "PUT",
        &format!("/api/v1/tenants/tenant-a/work-recovery/{kind}/commands/{id}"),
        body,
        true,
    )
    .await
}
pub(crate) async fn view(app: &Router, kind: &str) -> (StatusCode, Value) {
    request(
        app,
        "GET",
        &format!("/api/v1/tenants/tenant-a/work-recovery/{kind}"),
        Value::Null,
        true,
    )
    .await
}
pub(crate) async fn grant(pool: &PgPool) {
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES('tenant-a','operator-a','operations.read'),('tenant-a','operator-a','operations.manage') ON CONFLICT DO NOTHING").execute(pool).await.unwrap();
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn controls_are_audited_idempotent_fenced_and_independently_authorized(pool: PgPool) {
    let app = setup(&pool).await;
    let pause = command("pause", 0, json!({}));
    assert_eq!(
        send(&app, "replay", "pause-a", pause.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(view(&app, "replay").await.0, StatusCode::FORBIDDEN);
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES('tenant-a','operator-a','operations.manage')").execute(&pool).await.unwrap();
    let (a, b) = tokio::join!(
        send(&app, "replay", "pause-a", pause.clone()),
        send(&app, "replay", "pause-a", pause.clone())
    );
    assert_eq!(a.0, StatusCode::OK);
    assert_eq!(a, b);
    assert_eq!(a.1["generation"], "1");
    assert_eq!(view(&app, "replay").await.0, StatusCode::FORBIDDEN); // manage implies no read
    grant(&pool).await;
    let resume = command("resume", 1, json!({}));
    let (x, y) = tokio::join!(
        send(&app, "replay", "resume-a", resume.clone()),
        send(&app, "replay", "resume-b", resume)
    );
    assert!(matches!(
        (x.0, y.0),
        (StatusCode::OK, StatusCode::CONFLICT) | (StatusCode::CONFLICT, StatusCode::OK)
    ));
    assert_eq!(send(&app, "replay", "pause-a", pause.clone()).await, a); // never restores old pause
    assert_eq!(
        view(&app, "replay").await.1["control"],
        json!({"generation":"2","paused":false})
    );
    assert_eq!(
        send(&app, "replay", "changed", pause).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(
            &app,
            "calculator",
            "pause-a",
            command("pause", 0, json!({}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert!(
        sqlx::query("UPDATE analytics.work_recovery_audit SET actor_id=actor_id")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM analytics.work_controls")
            .execute(&pool)
            .await
            .is_err()
    );
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/v1/tenants/tenant-b/work-recovery/replay",
            Value::Null,
            true
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    sqlx::raw_sql("INSERT INTO identity.actors(actor_id) VALUES('operator-b'); INSERT INTO identity.tenant_action_grants VALUES('tenant-a','operator-b','operations.manage');").execute(&pool).await.unwrap();
    let collision=sqlx::query("SELECT analytics.recover_work('tenant-a','operator-b','pause-a','replay','pause','{}',0,NULL)").execute(&pool).await.unwrap_err();
    assert_eq!(
        collision.as_database_error().unwrap().code().as_deref(),
        Some("23505")
    );
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='operations.manage'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        send(&app, "replay", "pause-a", command("pause", 0, json!({})))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let denied=sqlx::query("SELECT analytics.recover_work('tenant-a','operator-a','pause-a','replay','pause','{}',0,NULL)").execute(&pool).await.unwrap_err();
    assert_eq!(
        denied.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.work_recovery_audit")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn delivery_retry_and_pause_preserve_incident_and_reject_stale_completion(pool: PgPool) {
    let app = setup(&pool).await;
    grant(&pool).await;
    rule(&pool, "high", "50", 0).await;
    append_evidence(&pool, "e", boundary(&pool).await, Some("51")).await;
    assert!(
        PostgresAlertLifecycle::new(pool.clone())
            .advance_one()
            .await
            .unwrap()
    );
    let work = PostgresAlertDelivery::new(pool.clone());
    work.seed(&[9; 32]).await.unwrap();
    let old = work.claim(&[9; 32]).await.unwrap().unwrap();
    let target = json!({"destination":"09".repeat(32),"rule_id":"high","revision":"1","firing_version":old.firing_version.to_string()});
    assert_eq!(
        send(
            &app,
            "notification",
            "active",
            command("retry", old.generation, target.clone())
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    sqlx::query("UPDATE analytics.alert_notification_delivery SET attempts=5,lease_until=clock_timestamp()-interval '1 second',last_failure='transport'").execute(&pool).await.unwrap();
    let (_, prepared) = view(&app, "notification").await;
    assert_eq!(prepared["targets"][0]["target"], target);
    let retry = command("retry", old.generation, target.clone());
    let receipt = send(&app, "notification", "retry-a", retry.clone()).await;
    assert_eq!(receipt.0, StatusCode::OK);
    assert_eq!(receipt.1["generation"], "2");
    assert!(!work.finish(&old, Ok(())).await.unwrap());
    let lease = work.claim(&[9; 32]).await.unwrap().unwrap();
    assert_eq!(lease.generation, 3);
    assert_eq!(lease.starts_at, old.starts_at);
    assert_eq!(
        send(
            &app,
            "notification",
            "pause",
            command("pause", 0, json!({}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(work.finish(&lease, Ok(())).await.unwrap()); // pause is not drain
    work.seed(&[8; 32]).await.unwrap();
    sqlx::query("UPDATE analytics.alert_notification_delivery SET next_attempt_at=clock_timestamp()-interval '1 second'").execute(&pool).await.unwrap();
    assert!(work.claim(&[9; 32]).await.unwrap().is_none());
    assert_eq!(send(&app, "notification", "retry-a", retry).await, receipt); // cannot reset successful current cycle
    assert_eq!(
        send(
            &app,
            "notification",
            "retry-done",
            command("retry", 3, target)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(
            &app,
            "notification",
            "resume",
            command("resume", 1, json!({}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(work.claim(&[9; 32]).await.unwrap().is_some());
    let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM analytics.alert_notification_intents),(SELECT count(*) FROM analytics.alert_transitions),(SELECT count(*) FROM analytics.alert_notification_delivery)").fetch_one(&pool).await.unwrap();
    assert_eq!(counts, (1, 1, 1));
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn calculator_recovery_keeps_frozen_inputs_and_three_attempt_cycles(pool: PgPool) {
    let app = setup(&pool).await;
    grant(&pool).await;
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES('tenant-a','operator-a','analytics.manage')").execute(&pool).await.unwrap();
    let base = "/api/v1/tenants/tenant-a/calculator-configurations/cfg";
    let parameters = json!({"calculator":"mean","version":"1","scope":"asset-a","target":"result","first_start":0,"window_ms":3000,"lateness_ms":0,"minimum_samples":1});
    assert_eq!(
        request(
            &app,
            "PUT",
            &format!("{base}/revisions/1"),
            parameters,
            true
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &format!("{base}/activation"),
            json!({"expected_generation":0,"revision":1}),
            true
        )
        .await
        .0,
        StatusCode::OK
    );
    let work = PostgresCalculatorWork::new(pool.clone());
    assert_eq!(
        send(&app, "calculator", "pause", command("pause", 0, json!({})))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(work.seed().await.unwrap(), 0);
    assert_eq!(
        send(
            &app,
            "calculator",
            "resume",
            command("resume", 1, json!({}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(work.seed().await.unwrap(), 32);
    let old = work.claim().await.unwrap().unwrap();
    let preparation = PostgresCalculatorPreparation::new(pool.clone());
    let record = calculate_prepared(preparation.prepare(&old.request).await.unwrap()).unwrap();
    let original: Value =
        sqlx::query_scalar("SELECT to_jsonb(p) FROM analytics.prepared_evaluations p")
            .fetch_one(&pool)
            .await
            .unwrap();
    let target = json!({"id":old.request.evaluation.id().as_str()});
    assert_eq!(
        send(
            &app,
            "calculator",
            "active",
            command("retry", i64::from(old.generation), target.clone())
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    sqlx::query("UPDATE analytics.calculator_work SET attempts=3,lease_until_ms=0,status='exhausted' WHERE evaluation_id=$1").bind(old.request.evaluation.id().as_str()).execute(&pool).await.unwrap();
    let retry = command("retry", i64::from(old.generation), target.clone());
    let receipt = send(&app, "calculator", "retry-a", retry.clone()).await;
    assert_eq!(receipt.0, StatusCode::OK);
    for generation in 3..=5 {
        let lease = work.claim().await.unwrap().unwrap();
        assert_eq!(lease.generation, generation);
        assert_eq!(lease.request.evaluation, old.request.evaluation);
        preparation.prepare(&lease.request).await.unwrap();
        sqlx::query("UPDATE analytics.calculator_work SET lease_until_ms=0 WHERE evaluation_id=$1")
            .bind(old.request.evaluation.id().as_str())
            .execute(&pool)
            .await
            .unwrap();
    }
    assert!(work.complete(&old, &record).await.is_err());
    assert_eq!(send(&app, "calculator", "retry-a", retry).await, receipt);
    let retained: Value =
        sqlx::query_scalar("SELECT to_jsonb(p) FROM analytics.prepared_evaluations p")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(original, retained);
    assert_eq!(
        send(
            &app,
            "calculator",
            "retry-b",
            command("retry", 5, target.clone())
        )
        .await
        .0,
        StatusCode::OK
    );
    let current = work.claim().await.unwrap().unwrap();
    assert_eq!(current.generation, 7);
    assert_eq!(
        send(
            &app,
            "calculator",
            "pause-b",
            command("pause", 2, json!({}))
        )
        .await
        .0,
        StatusCode::OK
    );
    work.complete(&current, &record).await.unwrap();
    assert!(work.claim().await.unwrap().is_none());
    assert_eq!(
        send(&app, "calculator", "completed", command("retry", 7, target))
            .await
            .0,
        StatusCode::CONFLICT
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn alert_pause_and_command_validation_never_invent_recovery_evidence(pool: PgPool) {
    let app = setup(&pool).await;
    grant(&pool).await;
    rule(&pool, "high", "50", 0).await;
    append_evidence(&pool, "e", boundary(&pool).await, Some("51")).await;
    assert_eq!(
        send(
            &app,
            "alert_evaluation",
            "pause",
            command("pause", 0, json!({}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(
        !PostgresAlertLifecycle::new(pool.clone())
            .advance_one()
            .await
            .unwrap()
    );
    assert_eq!(
        send(
            &app,
            "alert_evaluation",
            "fake",
            command("retry", 0, json!({"id":"e"}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let mut forged = command("resume", 1, json!({}));
    forged["actor_id"] = json!("operator-b");
    assert_eq!(
        send(&app, "alert_evaluation", "forged", forged).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(
            &app,
            "alert_evaluation",
            "resume",
            command("resume", 1, json!({}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(
        PostgresAlertLifecycle::new(pool.clone())
            .advance_one()
            .await
            .unwrap()
    );
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        send(
            &app,
            "alert_evaluation",
            "resume",
            command("resume", 1, json!({}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn recovery_preparation_is_bounded_and_never_returns_endpoints_or_payloads(pool: PgPool) {
    let app = setup(&pool).await;
    grant(&pool).await;
    rule(&pool, "high", "50", 0).await;
    append_evidence(&pool, "e", boundary(&pool).await, Some("51")).await;
    assert!(
        PostgresAlertLifecycle::new(pool.clone())
            .advance_one()
            .await
            .unwrap()
    );
    let work = PostgresAlertDelivery::new(pool.clone());
    for byte in 0..33 {
        work.seed(&[byte; 32]).await.unwrap();
    }
    sqlx::query("UPDATE analytics.alert_notification_delivery SET attempts=5")
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = view(&app, "notification").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["targets"].as_array().unwrap().len(), 32);
    assert_eq!(body["truncated"], true);
    for forbidden in ["http:", "https:", "request", "pre_state", "threshold"] {
        assert!(!body.to_string().contains(forbidden));
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn browser_mutation_guard_is_applied_before_recovery_effects(pool: PgPool) {
    let _app = setup(&pool).await;
    grant(&pool).await;
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
    let app = router(
        pool.clone(),
        auth,
        MutationGuard::Browser {
            public_origin: "https://fleet.example".into(),
        },
    );
    let body = command("pause", 0, json!({}));
    assert_eq!(
        send(&app, "replay", "browser", body.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    let request = Request::put("/api/v1/tenants/tenant-a/work-recovery/replay/commands/browser")
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .header("origin", "https://fleet.example")
        .header("x-fleetiq-csrf", "1")
        .body(Body::from(body.to_string()))
        .unwrap();
    assert_eq!(app.oneshot(request).await.unwrap().status(), StatusCode::OK);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.work_recovery_audit")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
