use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::Value;
use tower::ServiceExt;
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROOT: &str = "/api/v1/tenants/tenant-a/alert-rules/voltage-high";
fn authenticator(pool: &PgPool) -> impl RequestAuthenticator + Clone + use<> {
    let config = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "operator")
        .unwrap();
    fleetiq_access::CallerAuthenticationService::new(
        crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
            config.bearer_identity().unwrap().clone(),
        ),
        fleetiq_postgres::PostgresActorIdentityResolver::new(pool.clone()),
    )
}
async fn setup(pool: &PgPool, guard: MutationGuard) -> Router {
    sqlx::raw_sql(include_str!(
        "../../../postgres/tests/property_history/topology.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::raw_sql("INSERT INTO identity.tenant_action_grants VALUES ('tenant-a','operator-a','alerts.read'),('tenant-a','operator-a','alerts.manage');
      INSERT INTO metadata.property_definitions (tenant_id,property_id,version,display_name,value_kind,canonical_unit) VALUES ('tenant-a','mean.voltage',1,'Mean','decimal','volt');
      INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count) VALUES ('tenant-a','calculated',1,'Calculated',1);
      INSERT INTO metadata.asset_type_properties (tenant_id,type_id,type_version,property_id,property_version,property_ordinal) VALUES ('tenant-a','calculated',1,'mean.voltage',1,1);
      INSERT INTO metadata.assets (tenant_id,asset_id,type_id,type_version,display_name) VALUES ('tenant-a','result','calculated',1,'Result');
      INSERT INTO analytics.calculator_definitions VALUES ('tenant-a','mean','1','window-mean-v1','electrical.voltage',1,'mean.voltage',1);").execute(pool).await.unwrap();
    router(pool.clone(), authenticator(pool), guard)
}
fn definition() -> Value {
    json!({"kind":"numeric-threshold-v1","calculator":"mean","version":"1","scope":"asset-a","target":"result","output_property":"mean.voltage","output_version":1,"output_index":0,"comparison":"gt","threshold":"50","threshold_unit":"volt","hold_ms":1000,"resolution":"resolve_on_false","resolve_after_ms":2000})
}
fn request(method: &str, suffix: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(format!("{ROOT}{suffix}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}
async fn send(app: &Router, method: &str, suffix: &str, body: Value) -> (StatusCode, Value) {
    let r = app
        .clone()
        .oneshot(request(method, suffix, &body))
        .await
        .unwrap();
    assert_eq!(r.headers()["cache-control"], "no-store");
    let status = r.status();
    let body: Value =
        serde_json::from_slice(&to_bytes(r.into_body(), 4096).await.unwrap()).unwrap();
    if status == StatusCode::OK {
        let name = if suffix.is_empty() {
            "AlertRuleHead"
        } else if suffix == "/activation" {
            "AlertRuleActivationState"
        } else if method == "GET" {
            "AlertRuleRevision"
        } else {
            "AlertRuleRevisionReceipt"
        };
        let spec: Value =
            serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json"))
                .unwrap();
        let schema =
            json!({"components":spec["components"],"$ref":format!("#/components/schemas/{name}")});
        assert!(
            jsonschema::validator_for(&schema).unwrap().is_valid(&body),
            "{name} {body}"
        );
    }
    (status, body)
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn immutable_revisions_and_activation_retries_preserve_original_state(pool: PgPool) {
    let app = setup(&pool, MutationGuard::Workload).await;
    assert_eq!(
        send(&app, "GET", "", Value::Null).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(&app, "PUT", "/revisions/1", definition()).await.0,
        StatusCode::OK
    );
    let original = send(&app, "GET", "/revisions/1", Value::Null).await.1;
    assert_eq!(original["definition"], definition());
    assert_eq!(original["actor_id"], "operator-a");
    let mut equivalent = definition();
    equivalent["threshold"] = json!("50.00");
    assert_eq!(
        send(&app, "PUT", "/revisions/1", equivalent).await.0,
        StatusCode::OK
    );
    assert_eq!(
        send(&app, "GET", "/revisions/1", Value::Null).await.1,
        original
    );
    let head = send(&app, "GET", "", Value::Null).await.1;
    assert_eq!(head["latest_revision"], 1);
    assert_eq!(head["activation"]["generation"], 0);
    assert!(head["activation"]["revision"].is_null());
    let first = json!({"expected_generation":0,"revision":1});
    let receipt = send(&app, "PUT", "/activation", first.clone()).await.1;
    assert_eq!(receipt["generation"], 1);
    let mut changed = definition();
    changed["threshold"] = json!("51");
    assert_eq!(
        send(&app, "PUT", "/revisions/1", changed.clone()).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(&app, "PUT", "/revisions/3", changed.clone()).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(&app, "PUT", "/revisions/2", changed).await.0,
        StatusCode::OK
    );
    let head = send(&app, "GET", "", Value::Null).await.1;
    assert_eq!(head["latest_revision"], 2);
    assert_eq!(head["activation"]["revision"], 1);
    assert_eq!(
        send(
            &app,
            "PUT",
            "/activation",
            json!({"expected_generation":1,"revision":null})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(send(&app, "PUT", "/activation", first).await.1, receipt);
    assert!(send(&app, "GET", "", Value::Null).await.1["activation"]["revision"].is_null());
    assert_eq!(
        send(&app, "GET", "/revisions/1", Value::Null).await.1,
        original
    );
    assert!(
        sqlx::query("UPDATE analytics.alert_rule_revisions SET definition=definition")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM analytics.alert_rule_activation_changes")
            .execute(&pool)
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.calculator_work")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    sqlx::raw_sql("INSERT INTO identity.actors(actor_id) VALUES ('operator-b'); INSERT INTO identity.tenant_action_grants VALUES ('tenant-a','operator-b','alerts.manage'),('tenant-a','operator-b','alerts.read'); UPDATE identity.external_actor_identities SET actor_id='operator-b' WHERE authority_id='workload' AND subject='operator';").execute(&pool).await.unwrap();
    assert_eq!(
        send(&app, "PUT", "/revisions/1", definition()).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(
            &app,
            "PUT",
            "/activation",
            json!({"expected_generation":0,"revision":1})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(&app, "GET", "/revisions/1", Value::Null).await.1["actor_id"],
        "operator-a"
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn competing_revisions_and_activation_generations_conflict(pool: PgPool) {
    let app = setup(&pool, MutationGuard::Workload).await;
    let mut changed = definition();
    changed["threshold"] = json!("51");
    let (a, b) = tokio::join!(
        send(&app, "PUT", "/revisions/1", definition()),
        send(&app, "PUT", "/revisions/1", changed)
    );
    assert!([a.0, b.0].contains(&StatusCode::OK));
    assert!([a.0, b.0].contains(&StatusCode::CONFLICT));
    let (a, b) = tokio::join!(
        send(
            &app,
            "PUT",
            "/activation",
            json!({"expected_generation":0,"revision":1})
        ),
        send(
            &app,
            "PUT",
            "/activation",
            json!({"expected_generation":0,"revision":null})
        )
    );
    assert!([a.0, b.0].contains(&StatusCode::OK));
    assert!([a.0, b.0].contains(&StatusCode::CONFLICT));
    assert_eq!(
        send(
            &app,
            "PUT",
            "/activation",
            json!({"expected_generation":1,"revision":999})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(&app, "GET", "", Value::Null).await.1["activation"]["generation"],
        1
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn invalid_grammar_and_unsupported_output_selectors_fail_closed(pool: PgPool) {
    let app = setup(&pool, MutationGuard::Workload).await;
    for (key, value) in [
        ("kind", json!("promql")),
        ("comparison", json!("value > threshold")),
        ("threshold", json!("NaN")),
        ("threshold", json!("1e128")),
        ("threshold", json!("1e-65")),
        ("hold_ms", json!(604_800_001)),
        ("resolve_after_ms", json!(604_800_001)),
        ("resolution", json!("manual")),
        ("actor_id", json!("forged")),
    ] {
        let mut d = definition();
        d[key] = value;
        assert_eq!(
            send(&app, "PUT", "/revisions/1", d).await.0,
            StatusCode::BAD_REQUEST,
            "{key}"
        );
    }
    let mut omitted = definition();
    omitted.as_object_mut().unwrap().remove("threshold_unit");
    assert_eq!(
        send(&app, "PUT", "/revisions/1", omitted).await.0,
        StatusCode::BAD_REQUEST
    );
    for (key, value) in [
        ("output_index", json!(1)),
        ("output_property", json!("electrical.voltage")),
        ("output_version", json!(2)),
        ("target", json!("asset-a")),
        ("version", json!("missing")),
        ("threshold_unit", json!("ampere")),
    ] {
        let mut d = definition();
        d[key] = value;
        assert_eq!(
            send(&app, "PUT", "/revisions/1", d).await.0,
            StatusCode::CONFLICT,
            "{key}"
        );
    }
    for (field, value) in [("threshold_unit", Value::Null), ("hold_ms", json!(0.5))] {
        let mut invalid = definition();
        invalid[field] = value;
        assert!(
            sqlx::query(
                "SELECT analytics.create_alert_rule_revision('tenant-a','direct',1,'operator-a',$1)"
            )
            .bind(invalid)
            .execute(&pool)
            .await
            .is_err()
        );
    }
    assert_eq!(
        send(&app, "GET", "", Value::Null).await.0,
        StatusCode::NOT_FOUND
    );
    for body in [
        json!({"expected_generation":0}),
        json!({"expected_generation":0,"revision":0}),
        json!({"expected_generation":0,"revision":1,"actor_id":"forged"}),
    ] {
        assert_eq!(
            send(&app, "PUT", "/activation", body).await.0,
            StatusCode::BAD_REQUEST
        );
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn explicit_permissions_tenant_ownership_and_browser_guards_apply(pool: PgPool) {
    let browser = setup(
        &pool,
        MutationGuard::Browser {
            public_origin: "https://fleet.example".into(),
        },
    )
    .await;
    assert_eq!(
        send(&browser, "PUT", "/revisions/1", definition()).await.0,
        StatusCode::FORBIDDEN
    );
    let mut valid = request("PUT", "/revisions/1", &definition());
    valid
        .headers_mut()
        .insert("origin", "https://fleet.example".parse().unwrap());
    valid
        .headers_mut()
        .insert("x-fleetiq-csrf", "1".parse().unwrap());
    assert_eq!(
        browser.oneshot(valid).await.unwrap().status(),
        StatusCode::OK
    );
    let app = router(pool.clone(), authenticator(&pool), MutationGuard::Workload);
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='alerts.manage'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(send(&app, "GET", "", Value::Null).await.0, StatusCode::OK);
    assert_eq!(
        send(&app, "PUT", "/revisions/2", definition()).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::raw_sql("DELETE FROM identity.tenant_action_grants WHERE action='alerts.read'; INSERT INTO identity.tenant_action_grants VALUES ('tenant-a','operator-a','alerts.manage'),('tenant-a','operator-a','metadata.read'),('tenant-a','operator-a','analytics.manage'),('tenant-b','operator-a','alerts.read'),('tenant-b','operator-a','alerts.manage');").execute(&pool).await.unwrap();
    assert_eq!(
        send(&app, "GET", "", Value::Null).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&app, "PUT", "/revisions/2", definition()).await.0,
        StatusCode::OK
    );
    for (method, suffix, expected) in [
        ("GET", "", StatusCode::NOT_FOUND),
        ("PUT", "/revisions/1", StatusCode::CONFLICT),
    ] {
        let mut foreign = request(method, suffix, &definition());
        *foreign.uri_mut() = format!("{ROOT}{suffix}")
            .replace("tenant-a", "tenant-b")
            .parse()
            .unwrap();
        assert_eq!(
            app.clone().oneshot(foreign).await.unwrap().status(),
            expected
        );
    }
    let anonymous = Request::builder().uri(ROOT).body(Body::empty()).unwrap();
    assert_eq!(
        app.oneshot(anonymous).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn storage_rechecks_revoked_authority_and_inactive_lifecycle(pool: PgPool) {
    let app = setup(&pool, MutationGuard::Workload).await;
    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {TOKEN}").parse().unwrap());
    let actor = protected_request::authenticate(&headers, &authenticator(&pool))
        .await
        .unwrap();
    let tenant = TenantId::new("tenant-a").unwrap();
    let auth = TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone()))
        .authorize::<ManageAlerts>(actor, tenant)
        .await
        .unwrap();
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='alerts.manage'")
        .execute(&pool)
        .await
        .unwrap();
    let d: Parameters = serde_json::from_value(definition()).unwrap();
    let id = AlertRuleId::new("voltage-high").unwrap();
    assert!(matches!(
        PostgresAlertRules::new(pool.clone())
            .create(auth, &id, 1, &d.definition().unwrap())
            .await,
        Err(AlertRuleError::Forbidden)
    ));
    sqlx::query("UPDATE metadata.tenants SET is_active=FALSE WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        send(&app, "GET", "", Value::Null).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE identity.actors SET is_active=FALSE")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        send(&app, "GET", "", Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn dimensionless_detector_selection_requires_an_explicit_null_unit(pool: PgPool) {
    let app = setup(&pool, MutationGuard::Workload).await;
    sqlx::raw_sql("INSERT INTO metadata.property_definitions(tenant_id,property_id,version,display_name,value_kind) VALUES ('tenant-a','anomaly',1,'Anomaly','decimal'); INSERT INTO metadata.asset_type_definitions(tenant_id,type_id,version,display_name,property_count) VALUES ('tenant-a','anomaly',1,'Anomaly',1); INSERT INTO metadata.asset_type_properties VALUES ('tenant-a','anomaly',1,'anomaly',1,1); INSERT INTO metadata.assets(tenant_id,asset_id,type_id,type_version,display_name) VALUES ('tenant-a','anomaly-result','anomaly',1,'Anomaly'); INSERT INTO analytics.calculator_definitions VALUES ('tenant-a','mad','1','window-mad-v1','electrical.voltage',1,'anomaly',1);").execute(&pool).await.unwrap();
    let mut d = definition();
    d["calculator"] = json!("mad");
    d["target"] = json!("anomaly-result");
    d["output_property"] = json!("anomaly");
    d["threshold"] = json!("0.5");
    assert_eq!(
        send(&app, "PUT", "/revisions/1", d.clone()).await.0,
        StatusCode::CONFLICT
    );
    d["threshold_unit"] = Value::Null;
    assert_eq!(
        send(&app, "PUT", "/revisions/1", d.clone()).await.0,
        StatusCode::OK
    );
    assert_eq!(
        send(&app, "GET", "/revisions/1", Value::Null).await.1["definition"],
        d
    );
}
