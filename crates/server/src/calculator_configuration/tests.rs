use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use fleetiq_analytics::{CalculatorPreparation, calculate_prepared};
use fleetiq_postgres::{PostgresCalculatorPreparation, PostgresCalculatorWork};
use serde_json::Value;
use tower::ServiceExt;
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROOT: &str = "/api/v1/tenants/tenant-a/calculator-configurations/cfg";
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
    sqlx::raw_sql("INSERT INTO identity.tenant_action_grants VALUES ('tenant-a','operator-a','analytics.manage'),('tenant-a','operator-a','metadata.read');
      INSERT INTO metadata.property_definitions (tenant_id,property_id,version,display_name,value_kind,canonical_unit) VALUES ('tenant-a','mean.voltage',1,'Mean','decimal','volt');
      INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count) VALUES ('tenant-a','calculated',1,'Calculated',1);
      INSERT INTO metadata.asset_type_properties (tenant_id,type_id,type_version,property_id,property_version,property_ordinal) VALUES ('tenant-a','calculated',1,'mean.voltage',1,1);
      INSERT INTO metadata.assets (tenant_id,asset_id,type_id,type_version,display_name) VALUES ('tenant-a','result','calculated',1,'Result');
      INSERT INTO analytics.calculator_definitions VALUES ('tenant-a','mean','1','window-mean-v1','electrical.voltage',1,'mean.voltage',1);").execute(pool).await.unwrap();
    router(pool.clone(), authenticator(pool), guard)
}
fn parameters() -> Value {
    json!({"calculator":"mean","version":"1","scope":"asset-a","target":"result","first_start":0,"window_ms":3000,"lateness_ms":0,"minimum_samples":1})
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
    let response = app
        .clone()
        .oneshot(request(method, suffix, &body))
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let status = response.status();
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    if status == StatusCode::OK {
        let name = if suffix == "/activation" {
            "CalculatorActivationState"
        } else if method == "GET" {
            "CalculatorConfigurationRevision"
        } else {
            "CalculatorRevisionReceipt"
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
async fn revisions_activation_and_claims_keep_original_evidence(pool: PgPool) {
    let app = setup(&pool, MutationGuard::Workload).await;
    assert_eq!(
        send(&app, "PUT", "/revisions/1", parameters()).await.0,
        StatusCode::OK
    );
    let original = send(&app, "GET", "/revisions/1", Value::Null).await.1;
    assert_eq!(original["parameters"], parameters());
    assert_eq!(original["actor_id"], "operator-a");
    assert_eq!(
        send(&app, "PUT", "/revisions/1", parameters()).await.0,
        StatusCode::OK
    );
    assert_eq!(
        send(&app, "GET", "/revisions/1", Value::Null).await.1,
        original
    );
    let work = PostgresCalculatorWork::new(pool.clone());
    assert_eq!(work.seed().await.unwrap(), 0);
    let first = json!({"expected_generation":0,"revision":1});
    let activated = send(&app, "PUT", "/activation", first.clone()).await.1;
    assert_eq!(activated["generation"], 1);
    assert_eq!(work.seed().await.unwrap(), 32);
    let lease = work.claim().await.unwrap().unwrap();
    let record = calculate_prepared(
        PostgresCalculatorPreparation::new(pool.clone())
            .prepare(&lease.request)
            .await
            .unwrap(),
    )
    .unwrap();
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
    assert_eq!(work.seed().await.unwrap(), 0);
    assert!(work.claim().await.unwrap().is_none());
    work.complete(&lease, &record).await.unwrap();
    assert_eq!(send(&app, "PUT", "/activation", first).await.1, activated);
    assert_eq!(
        send(&app, "GET", "/activation", Value::Null).await.1["revision"],
        Value::Null
    );
    let mut changed = parameters();
    changed["minimum_samples"] = json!(2);
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
    assert_eq!(
        send(
            &app,
            "PUT",
            "/activation",
            json!({"expected_generation":2,"revision":1})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        work.claim().await.unwrap().unwrap().request.minimum_samples,
        1
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn competing_activation_and_revision_commands_have_one_outcome(pool: PgPool) {
    let app = setup(&pool, MutationGuard::Workload).await;
    let mut changed = parameters();
    changed["minimum_samples"] = json!(2);
    let (a, b) = tokio::join!(
        send(&app, "PUT", "/revisions/1", parameters()),
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
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.calculator_schedules")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn authorization_bounds_tenant_and_browser_protection_fail_closed(pool: PgPool) {
    let app = setup(
        &pool,
        MutationGuard::Browser {
            public_origin: "https://fleet.example".into(),
        },
    )
    .await;
    assert_eq!(
        send(&app, "PUT", "/revisions/1", parameters()).await.0,
        StatusCode::FORBIDDEN
    );
    let mut command = request("PUT", "/revisions/1", &parameters());
    command
        .headers_mut()
        .insert("origin", "https://fleet.example".parse().unwrap());
    command
        .headers_mut()
        .insert("x-fleetiq-csrf", "1".parse().unwrap());
    assert_eq!(
        app.clone().oneshot(command).await.unwrap().status(),
        StatusCode::OK
    );
    let workload = router(pool.clone(), authenticator(&pool), MutationGuard::Workload);
    for body in [
        json!({"expected_generation":0}),
        json!({"expected_generation":0,"revision":0}),
        json!({"expected_generation":0,"revision":1,"actor_id":"forged"}),
    ] {
        assert_eq!(
            send(&workload, "PUT", "/activation", body).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    let mut invalid = parameters();
    invalid["window_ms"] = json!(0);
    assert_eq!(
        send(&workload, "PUT", "/revisions/2", invalid).await.0,
        StatusCode::BAD_REQUEST
    );
    let mut invalid = parameters();
    invalid["target"] = json!("asset-a");
    assert_eq!(
        send(&workload, "PUT", "/revisions/2", invalid).await.0,
        StatusCode::CONFLICT
    );
    let foreign = Request::builder()
        .method("PUT")
        .uri(format!("{ROOT}/revisions/1").replace("tenant-a", "tenant-b"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .body(Body::from(parameters().to_string()))
        .unwrap();
    assert_eq!(
        workload.clone().oneshot(foreign).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='analytics.manage'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES ('tenant-a','operator-a','metadata.manage'),('tenant-a','operator-a','protocol.manage')").execute(&pool).await.unwrap();
    assert_eq!(
        send(&workload, "PUT", "/revisions/2", parameters()).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&workload, "GET", "/revisions/1", Value::Null).await.0,
        StatusCode::OK
    );
    sqlx::query("UPDATE metadata.tenants SET is_active=FALSE WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        send(&workload, "GET", "/revisions/1", Value::Null).await.0,
        StatusCode::FORBIDDEN
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn revocation_between_authorization_and_storage_is_rechecked(pool: PgPool) {
    let _app = setup(&pool, MutationGuard::Workload).await;
    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {TOKEN}").parse().unwrap());
    let actor = protected_request::authenticate(&headers, &authenticator(&pool))
        .await
        .unwrap();
    let auth = TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone()))
        .authorize::<ManageAnalytics>(actor, TenantId::new("tenant-a").unwrap())
        .await
        .unwrap();
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='analytics.manage'")
        .execute(&pool)
        .await
        .unwrap();
    let id = CalculatorConfigurationId::new("cfg").unwrap();
    let p: Parameters = serde_json::from_value(parameters()).unwrap();
    assert!(matches!(
        PostgresCalculatorConfiguration::new(pool)
            .create(
                auth,
                &id,
                1,
                &p.schedule(TenantId::new("tenant-a").unwrap(), id.clone())
                    .unwrap()
            )
            .await,
        Err(CalculatorConfigurationError::Forbidden)
    ));
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn deactivation_lock_excludes_new_claims_and_enumeration(pool: PgPool) {
    let app = setup(&pool, MutationGuard::Workload).await;
    assert_eq!(
        send(&app, "PUT", "/revisions/1", parameters()).await.0,
        StatusCode::OK
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
        StatusCode::OK
    );
    let work = PostgresCalculatorWork::new(pool.clone());
    work.seed().await.unwrap();
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "SELECT analytics.change_configuration_activation('tenant-a','cfg',1,NULL,'operator-a')",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert!(work.claim().await.unwrap().is_none());
    assert_eq!(work.seed().await.unwrap(), 0);
    transaction.commit().await.unwrap();
    assert!(work.claim().await.unwrap().is_none());
}
