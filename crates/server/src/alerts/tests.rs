use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use tower::ServiceExt;
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROOT: &str = "/api/v1/tenants/tenant-a/alerts";
async fn setup(pool: &PgPool) -> Router {
    sqlx::raw_sql(include_str!(
        "../../../postgres/tests/property_history/topology.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::raw_sql("INSERT INTO identity.tenant_action_grants VALUES ('tenant-a','operator-a','alerts.read'),('tenant-a','operator-a','alerts.manage'),('tenant-a','operator-a','metadata.read'),('tenant-b','operator-a','alerts.read');
      INSERT INTO metadata.property_definitions (tenant_id,property_id,version,display_name,value_kind,canonical_unit) VALUES ('tenant-a','mean.voltage',1,'Mean','decimal','volt');
      INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count) VALUES ('tenant-a','calculated',1,'Calculated',1);
      INSERT INTO metadata.asset_type_properties (tenant_id,type_id,type_version,property_id,property_version,property_ordinal) VALUES ('tenant-a','calculated',1,'mean.voltage',1,1);
      INSERT INTO metadata.assets (tenant_id,asset_id,type_id,type_version,display_name) VALUES ('tenant-a','result','calculated',1,'Result');
      INSERT INTO analytics.calculator_definitions VALUES ('tenant-a','mean','1','window-mean-v1','electrical.voltage',1,'mean.voltage',1);").execute(pool).await.unwrap();
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
fn definition(threshold: &str, hold: u32) -> Value {
    json!({"kind":"numeric-threshold-v1","calculator":"mean","version":"1","scope":"asset-a","target":"result","output_property":"mean.voltage","output_version":1,"output_index":0,"comparison":"gt","threshold":threshold,"threshold_unit":"volt","hold_ms":hold,"resolution":"resolve_on_false","resolve_after_ms":0})
}
async fn rule(pool: &PgPool, id: &str, threshold: &str, hold: u32) {
    sqlx::query("SELECT analytics.create_alert_rule_revision('tenant-a',$1,1,'operator-a',$2)")
        .bind(id)
        .bind(definition(threshold, hold))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("SELECT analytics.change_alert_rule_activation('tenant-a',$1,0,1,'operator-a')")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}
async fn boundary(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT GREATEST(max(recorded_at_ms),9007199254740993::bigint) FROM analytics.alert_rule_activation_changes").fetch_one(pool).await.unwrap()
}
async fn publish(pool: &PgPool, id: &str, start: i64, value: Option<&str>) {
    let outputs=value.map_or_else(||json!([]),|v|json!([{"asset":"result","property":"mean.voltage","version":1,"kind":"decimal","value":v,"unit":"volt"}]));
    let key = json!({"scope":"asset-a","calculator":"mean","version":"1","configuration":id,"start":start,"end":start+1000,"finalize":start+1000,"known":start+1000,"inputs":[]});
    sqlx::query("INSERT INTO analytics.evaluations(tenant_id,evaluation_id,replay_key,key_digest,evaluated_at_ms,outcome,outputs,minimum_samples) VALUES('tenant-a',$1,$2,sha256(convert_to($2::jsonb::text,'UTF8')),$3,$4,$5,1)")
        .bind(id).bind(key).bind(start+1000).bind(if value.is_some(){"complete"}else{"insufficient"}).bind(outputs).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO analytics.calculator_schedules(tenant_id,schedule_id,calculator_id,calculator_version,configuration_id,scope_asset_id,target_asset_id,first_start_ms,window_ms,lateness_ms,minimum_samples,next_start_ms) VALUES('tenant-a',$1,'mean','1',$1,'asset-a','result',$2,1000,0,1,$2)")
        .bind(id).bind(start).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO analytics.calculator_work(tenant_id,evaluation_id,schedule_id,start_ms,request,generation,status) VALUES('tenant-a',$1,$1,$2,'{\"target\":\"result\"}',1,'done')").bind(id).bind(start).execute(pool).await.unwrap();
    let work = fleetiq_postgres::PostgresAlertLifecycle::new(pool.clone());
    while work.advance_one().await.unwrap() {}
}
async fn response(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let status = response.status();
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1_048_576).await.unwrap()).unwrap();
    if status.is_client_error() || status.is_server_error() {
        let spec: Value =
            serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json"))
                .unwrap();
        let schema = json!({"components":spec["components"],"$ref":"#/components/schemas/Problem"});
        assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&body));
    }
    (status, body)
}
async fn get(app: &Router, suffix: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .uri(format!("{ROOT}{suffix}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, body) = response(app, request).await;
    if status == StatusCode::OK {
        let name = if suffix.contains("/transitions") {
            "AlertTransitionPage"
        } else if suffix.starts_with('/') {
            "AlertDetail"
        } else {
            "AlertPage"
        };
        let spec: Value =
            serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json"))
                .unwrap();
        let schema =
            json!({"components":spec["components"],"$ref":format!("#/components/schemas/{name}")});
        assert!(
            jsonschema::validator_for(&schema).unwrap().is_valid(&body),
            "{name}: {body}"
        );
    }
    (status, body)
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn exact_detail_and_history_preserve_pinned_rule_and_evidence(pool: PgPool) {
    let app = setup(&pool).await;
    rule(&pool, "high", "100.000", 1000).await;
    let base = boundary(&pool).await;
    for (i, value) in [Some("150"), Some("150"), Some("50")]
        .into_iter()
        .enumerate()
    {
        publish(
            &pool,
            &format!("e{i}"),
            base + i64::try_from(i).unwrap() * 1000,
            value,
        )
        .await;
    }
    assert_eq!(get(&app, "").await.1["alerts"], json!([]));
    let exact = get(&app, "/high/revisions/1").await;
    assert_eq!(exact.0, StatusCode::OK);
    assert_eq!(exact.1["definition"], definition("100", 1000));
    let alert = &exact.1["alert"];
    assert_eq!(alert["phase"], "resolved");
    assert_eq!(alert["version"], "3");
    assert_eq!(alert["last_event_at_ms"], (base + 3000).to_string());
    assert_eq!(
        alert["latest_transition"]["evidence"],
        json!({"evaluation_id":"e2","scope_asset_id":"asset-a","output_index":0})
    );
    let evidence_path = "/api/v1/tenants/tenant-a/assets/asset-a/calculator-evaluations/e2";
    let request = Request::builder()
        .uri(evidence_path)
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let evidence = response(&app, request).await;
    assert_eq!(evidence.0, StatusCode::OK);
    assert_eq!(evidence.1["evaluation"]["evaluation_id"], "e2");
    let request = Request::builder()
        .uri(format!("{evidence_path}/outputs"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(response(&app, request).await.1["outputs"][0]["value"], "50");
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a' AND action='telemetry.read'").execute(&pool).await.unwrap();
    let request = Request::builder()
        .uri(evidence_path)
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(response(&app, request).await.0, StatusCode::FORBIDDEN);
    assert_eq!(get(&app, "/high/revisions/1").await.0, StatusCode::OK);
    let mut suffix = "/high/revisions/1/transitions?limit=1".to_owned();
    let mut versions = vec![];
    loop {
        let page = get(&app, &suffix).await;
        assert_eq!(page.0, StatusCode::OK);
        versions.push(
            page.1["transitions"][0]["version"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
        if let Some(next) = page.1["next_after"].as_str() {
            suffix = format!("/high/revisions/1/transitions?limit=1&after={next}");
        } else {
            break;
        }
    }
    assert_eq!(versions, ["1", "2", "3"]);
    sqlx::query(
        "SELECT analytics.change_alert_rule_activation('tenant-a','high',1,NULL,'operator-a')",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        get(&app, "/high/revisions/1").await.1["alert"]["selected"],
        false
    );
    assert_eq!(
        get(&app, "?phase=resolved").await.1["alerts"][0]["rule_id"],
        "high"
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn queue_keysets_remain_stable_when_filter_membership_and_inserts_change(pool: PgPool) {
    let app = setup(&pool).await;
    for (id, threshold) in [("b", "0"), ("d", "100"), ("f", "0")] {
        rule(&pool, id, threshold, 0).await;
    }
    let base = boundary(&pool).await;
    publish(&pool, "first", base, Some("150")).await;
    let first = get(&app, "?limit=1").await;
    assert_eq!(first.1["alerts"][0]["rule_id"], "b");
    let token = first.1["next_after"].as_str().unwrap();
    let request = Request::builder()
        .uri(format!("/api/v1/tenants/tenant-b/alerts?after={token}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(response(&app, request).await.0, StatusCode::BAD_REQUEST);
    for id in ["a", "e"] {
        rule(&pool, id, "0", 0).await;
    }
    publish(&pool, "next", base + 1000, Some("50")).await;
    let page = get(&app, &format!("?limit=2&after={token}")).await;
    let ids: Vec<_> = page.1["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["rule_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["e", "f"]);
    assert!(page.1["next_after"].is_null());
    assert_eq!(
        get(&app, &format!("?phase=resolved&after={token}")).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get(&app, &format!("/b/revisions/1/transitions?after={token}"))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn empty_unobserved_missing_and_unknown_evidence_are_distinct(pool: PgPool) {
    let app = setup(&pool).await;
    assert_eq!(
        get(&app, "").await.1,
        json!({"alerts":[],"next_after":null})
    );
    rule(&pool, "high", "0", 1000).await;
    for suffix in [
        "/high/revisions/1",
        "/high/revisions/1/transitions",
        "/absent/revisions/1",
    ] {
        assert_eq!(get(&app, suffix).await.0, StatusCode::NOT_FOUND);
    }
    let base = boundary(&pool).await;
    publish(&pool, "pending", base, Some("1")).await;
    publish(&pool, "unknown", base + 1000, None).await;
    let exact = get(&app, "/high/revisions/1").await.1;
    assert_eq!(exact["alert"]["phase"], "inactive");
    assert!(exact["alert"]["latest_transition"]["evidence"]["output_index"].is_null());
    assert_eq!(
        get(&app, "?phase=inactive").await.1["alerts"][0]["rule_id"],
        "high"
    );
    let key = Key {
        scope: scope(("alert-transitions-v1", "tenant-a", "high", 1_u32)).unwrap(),
        rule: "high".into(),
        revision: 1,
        version: i64::MAX,
    };
    let token = cursor_codec::encode(&key).unwrap();
    assert_eq!(
        get(
            &app,
            &format!("/high/revisions/1/transitions?after={token}")
        )
        .await
        .1,
        json!({"transitions":[],"next_after":null})
    );
    sqlx::query("UPDATE analytics.alert_lifecycles SET phase='firing' WHERE rule_id='high'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        get(&app, "/high/revisions/1").await.0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn permission_and_tenant_checks_precede_feature_discovery(pool: PgPool) {
    let app = setup(&pool).await;
    rule(&pool, "high", "0", 0).await;
    let base = boundary(&pool).await;
    publish(&pool, "one", base, Some("1")).await;
    for suffix in ["", "/high/revisions/1", "/high/revisions/1/transitions"] {
        let request = Request::builder()
            .uri(format!("{ROOT}{suffix}").replace("tenant-a", "tenant-b"))
            .header("authorization", format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();
        let result = response(&app, request).await;
        assert_eq!(
            result.0,
            if suffix.is_empty() {
                StatusCode::OK
            } else {
                StatusCode::NOT_FOUND
            }
        );
        if suffix.is_empty() {
            assert_eq!(result.1, json!({"alerts":[],"next_after":null}));
        }
    }
    let request = Request::builder().uri(ROOT).body(Body::empty()).unwrap();
    assert_eq!(response(&app, request).await.0, StatusCode::UNAUTHORIZED);
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='alerts.read' AND tenant_id='tenant-a'").execute(&pool).await.unwrap();
    for suffix in [
        "?phase=bogus&limit=0",
        "/high/revisions/bad",
        "/absent/revisions/1/transitions?after=bad",
    ] {
        assert_eq!(get(&app, suffix).await.0, StatusCode::FORBIDDEN);
    }
    sqlx::query(
        "INSERT INTO identity.tenant_action_grants VALUES('tenant-a','operator-a','alerts.read')",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(get(&app, "").await.0, StatusCode::OK);
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(get(&app, "").await.0, StatusCode::FORBIDDEN);
    sqlx::query("UPDATE identity.actors SET is_active=false WHERE actor_id='operator-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(get(&app, "").await.0, StatusCode::UNAUTHORIZED);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn strict_query_cursor_identity_and_method_bounds_fail_closed(pool: PgPool) {
    let app = setup(&pool).await;
    rule(&pool, "high", "0", 1000).await;
    let base = boundary(&pool).await;
    publish(&pool, "first", base, Some("1")).await;
    publish(&pool, "second", base + 1000, Some("1")).await;
    for suffix in [
        "?phase=unknown",
        "?limit=0",
        "?limit=101",
        "?limit=-1",
        "?limit=65536",
        "?extra=1",
        "?phase=firing&phase=resolved",
        "?after=bad",
        "/high/revisions/0",
        "/high/revisions/01",
        "/high/revisions/4294967296",
        "/high/revisions/1?extra=1",
        "/high/revisions/1/transitions?limit=101",
    ] {
        assert_eq!(
            get(&app, suffix).await.0,
            StatusCode::BAD_REQUEST,
            "{suffix}"
        );
    }
    assert_eq!(
        get(&app, &format!("?after={}", "a".repeat(8193))).await.0,
        StatusCode::BAD_REQUEST
    );
    let history = get(&app, "/high/revisions/1/transitions?limit=1").await.1;
    let token = history["next_after"].as_str().unwrap();
    for suffix in [
        format!("?after={token}"),
        format!("/high/revisions/2/transitions?after={token}"),
    ] {
        assert_eq!(get(&app, &suffix).await.0, StatusCode::BAD_REQUEST);
    }
    let mut decoded: Key = cursor_codec::decode(token).unwrap();
    decoded.version = 0;
    assert_eq!(
        get(
            &app,
            &format!(
                "/high/revisions/1/transitions?after={}",
                cursor_codec::encode(&decoded).unwrap()
            )
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let request = Request::builder()
        .method("POST")
        .uri(ROOT)
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let r = app.clone().oneshot(request).await.unwrap();
    assert_eq!(r.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(r.headers()["allow"], "GET, HEAD");
    assert_eq!(r.headers()["cache-control"], "no-store");
    let request = Request::builder()
        .method("HEAD")
        .uri(ROOT)
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let r = app.clone().oneshot(request).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert!(to_bytes(r.into_body(), 1_048_576).await.unwrap().is_empty());
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn full_page_c_collation_and_empty_tail_preserve_all_identities(pool: PgPool) {
    let app = setup(&pool).await;
    let mut expected = vec!["é".to_owned(), "a".into(), "Z".into()];
    expected.extend((0..98).map(|n| format!("r{n:03}")));
    for id in &expected {
        rule(&pool, id, "0", 0).await;
    }
    let base = boundary(&pool).await;
    publish(&pool, "one", base, Some("1")).await;
    let first = get(&app, "?limit=100").await.1;
    assert_eq!(first["alerts"].as_array().unwrap().len(), 100);
    let next = first["next_after"].as_str().unwrap();
    let second = get(&app, &format!("?limit=100&after={next}")).await.1;
    assert_eq!(second["alerts"].as_array().unwrap().len(), 1);
    assert!(second["next_after"].is_null());
    let mut ids = first["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["rule_id"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    ids.push(second["alerts"][0]["rule_id"].as_str().unwrap().to_owned());
    expected.sort();
    assert_eq!(ids, expected);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn browser_session_composition_and_internal_failure_are_bounded(pool: PgPool) {
    let bearer = setup(&pool).await;
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
    sqlx::query("INSERT INTO identity.browser_sessions(credential_digest,provider_digest,authority_id,subject,actor_id,expires_at_ms) VALUES(encode(sha256(convert_to($1,'UTF8')),'hex'),encode(sha256(convert_to($2,'UTF8')),'hex'),'workload','operator','operator-a',floor(extract(epoch FROM clock_timestamp())*1000)::bigint+60000)")
        .bind(TOKEN).bind(binding).execute(&pool).await.unwrap();
    let auth = crate::browser_identity::BrowserRequestAuthenticator::new(pool.clone(), &config);
    let app = crate::protected_application_router(
        pool.clone(),
        auth,
        crate::mutation_guard::MutationGuard::Browser {
            public_origin: "https://fleet.example".into(),
        },
    );
    let request = Request::builder()
        .uri(ROOT)
        .header("cookie", format!("__Host-fleetiq-session={TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let r = response(&app, request).await;
    assert_eq!(r.0, StatusCode::OK);
    assert_eq!(r.1, json!({"alerts":[],"next_after":null}));
    let request = Request::builder().uri(ROOT).body(Body::empty()).unwrap();
    assert_eq!(response(&app, request).await.0, StatusCode::UNAUTHORIZED);
    pool.close().await;
    let r = get(&bearer, "").await;
    assert_eq!(r.0, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        r.1,
        json!({"type":"urn:fleetiq:problem:internal-error","title":"Internal server error","status":500})
    );
}
