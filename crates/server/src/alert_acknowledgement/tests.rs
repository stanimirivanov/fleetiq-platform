use super::*;
use crate::alerts::tests::{boundary, get, publish, rule, setup};
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use tower::ServiceExt;
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROOT: &str = "/api/v1/tenants/tenant-a/alerts/high/revisions/1/acknowledgement";
async fn grant(pool: &PgPool) {
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES('tenant-a','operator-a','alerts.acknowledge')").execute(pool).await.unwrap();
}
async fn send(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("PUT")
        .uri(uri)
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    response(app, request).await
}
async fn response(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let r = app.clone().oneshot(request).await.unwrap();
    assert_eq!(r.headers()["cache-control"], "no-store");
    let status = r.status();
    let body: Value =
        serde_json::from_slice(&to_bytes(r.into_body(), 4096).await.unwrap()).unwrap();
    let spec: Value =
        serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json")).unwrap();
    let schema = json!({"components":spec["components"],"$ref":if status==StatusCode::OK {"#/components/schemas/AlertAcknowledgement"}else{"#/components/schemas/Problem"}});
    assert!(
        jsonschema::validator_for(&schema).unwrap().is_valid(&body),
        "{body}"
    );
    (status, body)
}
fn command(v: &str) -> Value {
    json!({"expected_version":v})
}
async fn snapshot(pool: &PgPool) -> (Value, i64, i64) {
    sqlx::query_as("SELECT to_jsonb(a),(SELECT count(*) FROM analytics.alert_transitions),(SELECT count(*) FROM analytics.alert_notification_intents) FROM analytics.alert_lifecycles a WHERE tenant_id='tenant-a' AND rule_id='high' AND revision=1").fetch_one(pool).await.unwrap()
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn acknowledgement_pins_original_author_time_and_never_changes_condition_or_intents(
    pool: PgPool,
) {
    let app = setup(&pool).await;
    grant(&pool).await;
    rule(&pool, "high", "0", 0).await;
    let base = boundary(&pool).await;
    publish(&pool, "fire", base, Some("1")).await;
    let before = snapshot(&pool).await;
    let first = send(&app, ROOT, command("1")).await;
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(first.1["actor_id"], "operator-a");
    assert_eq!(send(&app, ROOT, command("1")).await, first);
    assert_eq!(snapshot(&pool).await, before);
    assert_eq!(
        get(&app, "/high/revisions/1").await.1["alert"]["latest_transition"]["acknowledgement"],
        first.1
    );
    assert_eq!(
        get(&app, "?phase=firing").await.1["alerts"][0]["latest_transition"]["acknowledgement"],
        first.1
    );
    publish(&pool, "resolve", base + 1000, Some("0")).await;
    assert_eq!(send(&app, ROOT, command("1")).await, first);
    assert_eq!(send(&app, ROOT, command("2")).await.0, StatusCode::CONFLICT);
    let result = get(&app, "/high/revisions/1/transitions").await;
    assert_eq!(result.0, StatusCode::OK, "{}", result.1);
    let history = result.1;
    assert_eq!(history["transitions"][0]["acknowledgement"], first.1);
    assert!(history["transitions"][1]["acknowledgement"].is_null());
    assert_eq!(snapshot(&pool).await.1, 2);
    assert_eq!(snapshot(&pool).await.2, 2);
    for sql in [
        "DELETE FROM analytics.alert_acknowledgements",
        "UPDATE analytics.alert_acknowledgements SET actor_id='operator-a'",
    ] {
        assert!(sqlx::query(sql).execute(&pool).await.is_err());
    }
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='alerts.read'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(send(&app, ROOT, command("1")).await.0, StatusCode::OK);
    assert_eq!(get(&app, "").await.0, StatusCode::FORBIDDEN);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn strict_input_and_explicit_authority_precede_discovery(pool: PgPool) {
    let app = setup(&pool).await;
    rule(&pool, "high", "0", 0).await;
    assert_eq!(
        send(&app, ROOT, command("1")).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&app, ROOT, json!({"actor_id":"forged"})).await.0,
        StatusCode::FORBIDDEN
    );
    grant(&pool).await;
    for body in [
        json!({}),
        json!({"expected_version":1}),
        command("0"),
        command("01"),
        command("-1"),
        command("9223372036854775808"),
        json!({"expected_version":"1","actor_id":"forged"}),
    ] {
        assert_eq!(send(&app, ROOT, body).await.0, StatusCode::BAD_REQUEST);
    }
    assert_eq!(
        send(&app, ROOT, json!({"expected_version":"a".repeat(2000)}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(&app, &format!("{ROOT}?extra=1"), command("1")).await.0,
        StatusCode::BAD_REQUEST
    );
    for uri in [
        ROOT.to_owned(),
        ROOT.replace("high", "absent"),
        ROOT.replace("revisions/1", "revisions/2"),
    ] {
        assert_eq!(send(&app, &uri, command("1")).await.0, StatusCode::CONFLICT);
    }
    let base = boundary(&pool).await;
    publish(&pool, "fire", base, Some("1")).await;
    assert_eq!(send(&app, ROOT, command("2")).await.0, StatusCode::CONFLICT);
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES('tenant-b','operator-a','alerts.acknowledge')").execute(&pool).await.unwrap();
    assert_eq!(
        send(&app, &ROOT.replace("tenant-a", "tenant-b"), command("1"))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let request = Request::builder()
        .method("PUT")
        .uri(ROOT)
        .header("content-type", "application/json")
        .body(Body::from(command("1").to_string()))
        .unwrap();
    assert_eq!(response(&app, request).await.0, StatusCode::UNAUTHORIZED);
    let r = app
        .clone()
        .oneshot(Request::builder().uri(ROOT).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(r.headers()["allow"], "PUT");
}
fn authenticated(pool: &PgPool, subject: &str) -> impl RequestAuthenticator + Clone + use<> {
    let config = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", subject)
        .unwrap();
    fleetiq_access::CallerAuthenticationService::new(
        crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
            config.bearer_identity().unwrap().clone(),
        ),
        fleetiq_postgres::PostgresActorIdentityResolver::new(pool.clone()),
    )
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn same_author_retries_and_different_actor_races_have_one_original_receipt(pool: PgPool) {
    let app = setup(&pool).await;
    grant(&pool).await;
    rule(&pool, "high", "0", 0).await;
    let base = boundary(&pool).await;
    publish(&pool, "fire", base, Some("1")).await;
    let (a, b) = tokio::join!(
        send(&app, ROOT, command("1")),
        send(&app, ROOT, command("1"))
    );
    assert_eq!(a.0, StatusCode::OK);
    assert_eq!(a, b);
    sqlx::raw_sql("INSERT INTO identity.actors(actor_id) VALUES('operator-b'); INSERT INTO identity.external_actor_identities(authority_id,subject,actor_id) VALUES('workload','second','operator-b'); INSERT INTO identity.tenant_action_grants VALUES('tenant-a','operator-b','alerts.acknowledge');").execute(&pool).await.unwrap();
    let other = router(
        pool.clone(),
        authenticated(&pool, "second"),
        MutationGuard::Workload,
    );
    assert_eq!(
        send(&other, ROOT, command("1")).await.0,
        StatusCode::CONFLICT
    );
    publish(&pool, "resolve", base + 1000, Some("0")).await;
    publish(&pool, "refire", base + 2000, Some("1")).await;
    let (a, b) = tokio::join!(
        send(&app, ROOT, command("3")),
        send(&other, ROOT, command("3"))
    );
    assert!([a.0, b.0].contains(&StatusCode::OK));
    assert!([a.0, b.0].contains(&StatusCode::CONFLICT));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.alert_acknowledgements")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn acknowledgement_resolve_race_serializes_at_the_checkpoint(pool: PgPool) {
    let app = setup(&pool).await;
    grant(&pool).await;
    rule(&pool, "high", "0", 0).await;
    let base = boundary(&pool).await;
    publish(&pool, "fire", base, Some("1")).await;
    crate::alerts::tests::append_evidence(&pool, "resolve", base + 1000, Some("0")).await;
    let mut barrier = pool.begin().await.unwrap();
    sqlx::query("SELECT version FROM analytics.alert_lifecycles FOR UPDATE")
        .execute(&mut *barrier)
        .await
        .unwrap();
    let release = async {
        tokio::time::timeout(std::time::Duration::from_secs(5),async {
            loop {
                let blocked:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock'").fetch_one(&pool).await.unwrap();
                if blocked>=2 {break;}tokio::task::yield_now().await;
            }
        }).await.unwrap();
        barrier.commit().await.unwrap();
    };
    let work = fleetiq_postgres::PostgresAlertLifecycle::new(pool.clone());
    let (ack, completed, ()) =
        tokio::join!(send(&app, ROOT, command("1")), work.advance_one(), release);
    assert!(completed.unwrap());
    assert!([StatusCode::OK, StatusCode::CONFLICT].contains(&ack.0));
    let state = snapshot(&pool).await;
    assert_eq!(state.0["phase"], "resolved");
    assert_eq!(state.0["version"], 2);
    assert_eq!((state.1, state.2), (2, 2));
    let receipts: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.alert_acknowledgements")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(receipts, i64::from(ack.0 == StatusCode::OK));
    if ack.0 == StatusCode::OK {
        assert_eq!(send(&app, ROOT, command("1")).await, ack);
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn captured_authority_and_old_retries_are_rechecked_on_revocation(pool: PgPool) {
    let app = setup(&pool).await;
    grant(&pool).await;
    rule(&pool, "high", "0", 0).await;
    let base = boundary(&pool).await;
    publish(&pool, "fire", base, Some("1")).await;
    assert_eq!(send(&app, ROOT, command("1")).await.0, StatusCode::OK);
    let authentication = authenticated(&pool, "operator");
    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {TOKEN}").parse().unwrap());
    let actor = authentication.authenticate(&headers).await.unwrap();
    let access = TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone()));
    let captured = access
        .authorize::<AcknowledgeAlerts>(actor, TenantId::new("tenant-a").unwrap())
        .await
        .unwrap();
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='alerts.acknowledge'")
        .execute(&pool)
        .await
        .unwrap();
    let result = PostgresAlertAcknowledgement::new(pool.clone())
        .acknowledge(captured, &AlertRuleId::new("high").unwrap(), 1, 1)
        .await;
    assert!(matches!(result, Err(AlertAcknowledgementError::Forbidden)));
    assert_eq!(
        send(&app, ROOT, command("1")).await.0,
        StatusCode::FORBIDDEN
    );
    grant(&pool).await;
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        send(&app, ROOT, command("1")).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE identity.actors SET is_active=false WHERE actor_id='operator-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        send(&app, ROOT, command("1")).await.0,
        StatusCode::UNAUTHORIZED
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn browser_origin_csrf_and_expired_session_protect_commands_and_retries(pool: PgPool) {
    let _ = setup(&pool).await;
    grant(&pool).await;
    rule(&pool, "high", "0", 0).await;
    let base = boundary(&pool).await;
    publish(&pool, "fire", base, Some("1")).await;
    let config = crate::BrowserIdentityConfig::from_values(
        "https://fleet.example",
        "https://identity.example",
        "client",
        "secret",
        "workload",
    )
    .unwrap();
    let binding = format!(
        "{}\n{}\n{}",
        config.issuer(),
        config.client_id(),
        config.identity_authority().as_str()
    );
    sqlx::query("INSERT INTO identity.browser_sessions VALUES(encode(sha256(convert_to($1,'UTF8')),'hex'),encode(sha256(convert_to($2,'UTF8')),'hex'),'workload','operator','operator-a',floor(extract(epoch FROM clock_timestamp())*1000)::bigint+60000)").bind(TOKEN).bind(binding).execute(&pool).await.unwrap();
    let app = crate::protected_application_router(
        pool.clone(),
        crate::browser_identity::BrowserRequestAuthenticator::new(pool.clone(), &config),
        MutationGuard::Browser {
            public_origin: "https://fleet.example".into(),
        },
    );
    let request = |origin: Option<&str>, csrf: Option<&str>| {
        let mut r = Request::builder()
            .method("PUT")
            .uri(ROOT)
            .header("cookie", format!("__Host-fleetiq-session={TOKEN}"))
            .header("content-type", "application/json");
        if let Some(value) = origin {
            r = r.header("origin", value);
        }
        if let Some(value) = csrf {
            r = r.header("x-fleetiq-csrf", value);
        }
        r.body(Body::from(command("1").to_string())).unwrap()
    };
    for (origin, csrf) in [
        (None, None),
        (Some("https://evil.example"), Some("1")),
        (Some("https://fleet.example"), None),
    ] {
        assert_eq!(
            response(&app, request(origin, csrf)).await.0,
            StatusCode::FORBIDDEN
        );
    }
    let accepted = response(&app, request(Some("https://fleet.example"), Some("1"))).await;
    assert_eq!(accepted.0, StatusCode::OK);
    let mut duplicate = request(Some("https://fleet.example"), Some("1"));
    duplicate
        .headers_mut()
        .append("origin", "https://fleet.example".parse().unwrap());
    assert_eq!(response(&app, duplicate).await.0, StatusCode::FORBIDDEN);
    sqlx::query("WITH expired AS (DELETE FROM identity.browser_sessions RETURNING credential_digest,provider_digest,authority_id,subject,actor_id) INSERT INTO identity.browser_sessions SELECT credential_digest,provider_digest,authority_id,subject,actor_id,1 FROM expired")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        response(&app, request(Some("https://fleet.example"), Some("1")))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn pending_acknowledgement_does_not_carry_to_a_new_firing_version(pool: PgPool) {
    let app = setup(&pool).await;
    grant(&pool).await;
    rule(&pool, "high", "0", 1000).await;
    let base = boundary(&pool).await;
    publish(&pool, "pending", base, Some("1")).await;
    let first = send(&app, ROOT, command("1")).await;
    assert_eq!(first.0, StatusCode::OK);
    publish(&pool, "fire", base + 1000, Some("1")).await;
    let detail = get(&app, "/high/revisions/1").await.1;
    assert_eq!(detail["alert"]["phase"], "firing");
    assert!(detail["alert"]["latest_transition"]["acknowledgement"].is_null());
    assert_eq!(send(&app, ROOT, command("1")).await, first);
    assert_eq!(send(&app, ROOT, command("2")).await.0, StatusCode::OK);
    let result = get(&app, "/high/revisions/1/transitions").await;
    assert_eq!(result.0, StatusCode::OK, "{}", result.1);
    let history = result.1;
    assert_eq!(history["transitions"][0]["acknowledgement"]["version"], "1");
    assert_eq!(history["transitions"][1]["acknowledgement"]["version"], "2");
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn failed_audit_insert_rolls_back_and_retry_preserves_open_state(pool: PgPool) {
    let app = setup(&pool).await;
    grant(&pool).await;
    rule(&pool, "high", "0", 0).await;
    let base = boundary(&pool).await;
    publish(&pool, "fire", base, Some("1")).await;
    let before = snapshot(&pool).await;
    sqlx::raw_sql("CREATE FUNCTION analytics.reject_ack_test() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected acknowledgement audit failure'; END $$; CREATE TRIGGER reject_ack_test BEFORE INSERT ON analytics.alert_acknowledgements FOR EACH ROW EXECUTE FUNCTION analytics.reject_ack_test();").execute(&pool).await.unwrap();
    let result = send(&app, ROOT, command("1")).await;
    assert_eq!(result.0, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        result.1,
        json!({"status":500,"title":"Internal server error","type":"urn:fleetiq:problem:internal-error"})
    );
    assert_eq!(snapshot(&pool).await, before);
    let receipts: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.alert_acknowledgements")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(receipts, 0);
    sqlx::query("DROP TRIGGER reject_ack_test ON analytics.alert_acknowledgements")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(send(&app, ROOT, command("1")).await.0, StatusCode::OK);
    assert_eq!(snapshot(&pool).await, before);
}
