//! Protected real-database export lifecycle and bounded transport regressions.
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use fleetiq_access::CallerAuthenticationService;
use fleetiq_postgres::PostgresActorIdentityResolver;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROOT: &str = "/api/v1/tenants/tenant-a/exports";
fn app(pool: &PgPool, browser: bool) -> Router {
    let config = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "operator")
        .unwrap();
    crate::export_routes::router(
        pool.clone(),
        CallerAuthenticationService::new(
            crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
                config.bearer_identity().unwrap().clone(),
            ),
            PostgresActorIdentityResolver::new(pool.clone()),
        ),
        if browser {
            crate::mutation_guard::MutationGuard::Browser {
                public_origin: "https://fleet.example".into(),
            }
        } else {
            crate::mutation_guard::MutationGuard::Workload
        },
    )
}
async fn setup(pool: &PgPool) -> Router {
    sqlx::raw_sql(include_str!(
        "../../../postgres/tests/property_history/topology.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::raw_sql("INSERT INTO identity.tenant_action_grants VALUES('tenant-a','operator-a','operations.manage'); INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES('tenant-a',1,'operator-a',7,30,4,16777216)").execute(pool).await.unwrap();
    app(pool, false)
}
fn body() -> Value {
    json!({"asset_id":"asset-a","property_id":"electrical.voltage","property_version":1,"start_at_ms":"0","end_at_ms":"3000"})
}
fn request(method: Method, path: &str, body: Option<Value>) -> Request<Body> {
    let r = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {TOKEN}"));
    let (r, b) = match body {
        Some(b) => (
            r.header("content-type", "application/json"),
            Body::from(b.to_string()),
        ),
        None => (r, Body::empty()),
    };
    r.body(b).unwrap()
}

async fn json_response(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let r = app
        .clone()
        .oneshot(request(method, path, body))
        .await
        .unwrap();
    let status = r.status();
    assert_eq!(r.headers()["cache-control"], "no-store");
    (
        status,
        serde_json::from_slice(&to_bytes(r.into_body(), 32768).await.unwrap()).unwrap(),
    )
}
async fn create(app: &Router) -> (String, Value) {
    let (s, b) = json_response(
        app,
        Method::PUT,
        &format!("{ROOT}/requests/test-one"),
        Some(body()),
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let id = b["export_id"].as_str().unwrap().to_owned();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let (s, b) = json_response(app, Method::GET, &format!("{ROOT}/jobs/{id}"), None).await;
            assert_eq!(s, StatusCode::OK);
            if b["state"] == "ready" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    (id, b)
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn creation_retry_status_download_and_cancellation_are_exact(pool: PgPool) {
    let app = setup(&pool).await;
    let (id, original) = create(&app).await;
    let contract: Value =
        serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json")).unwrap();
    let schema = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","components":{"schemas":contract["components"]["schemas"]},"$ref":"#/components/schemas/ExportJob"});
    assert!(
        jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(&original)
    );
    let (s, retry) = json_response(
        &app,
        Method::PUT,
        &format!("{ROOT}/requests/test-one"),
        Some(body()),
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    assert_eq!(retry["export_id"], id);
    assert_eq!(retry["expires_at_ms"], original["expires_at_ms"]);
    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("{ROOT}/jobs/{id}/download"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(
        response.headers()["content-type"],
        "application/vnd.apache.parquet"
    );
    assert_eq!(
        response.headers()["content-disposition"],
        format!("attachment; filename=\"fleetiq-export-{id}.parquet\"")
    );
    let length = response.headers()["content-length"]
        .to_str()
        .unwrap()
        .parse::<usize>()
        .unwrap();
    let bytes = to_bytes(response.into_body(), 2_097_152).await.unwrap();
    assert_eq!(bytes.len(), length);
    let rows = fleetiq_archive::decode(&fleetiq_domain::TenantId::new("tenant-a").unwrap(), &bytes)
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].observation().value(),
        &fleetiq_domain::PropertyValue::Integer(i64::MAX)
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.work_reservations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        json_response(&app, Method::DELETE, &format!("{ROOT}/jobs/{id}"), None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        json_response(
            &app,
            Method::GET,
            &format!("{ROOT}/jobs/{id}/download"),
            None
        )
        .await
        .0,
        StatusCode::GONE
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn authority_is_current_and_operational_mutations_are_independent(pool: PgPool) {
    let app = setup(&pool).await;
    let (id, _) = create(&app).await;
    assert_eq!(
        json_response(
            &app,
            Method::GET,
            &format!("{ROOT}/jobs/{id}/download").replace("tenant-a", "tenant-b"),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES('tenant-b','operator-a','telemetry.read')").execute(&pool).await.unwrap();
    assert_eq!(
        json_response(
            &app,
            Method::GET,
            &format!("{ROOT}/jobs/{id}/download").replace("tenant-a", "tenant-b"),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a' AND action='operations.manage'").execute(&pool).await.unwrap();
    assert_eq!(
        json_response(
            &app,
            Method::PUT,
            &format!("{ROOT}/requests/test-one"),
            Some(body())
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        json_response(&app, Method::DELETE, &format!("{ROOT}/jobs/{id}"), None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        json_response(&app, Method::GET, &format!("{ROOT}/jobs/{id}"), None)
            .await
            .0,
        StatusCode::OK
    );
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a' AND action='telemetry.read'").execute(&pool).await.unwrap();
    for suffix in [format!("/{id}"), format!("/{id}/download")] {
        assert_eq!(
            json_response(&app, Method::GET, &format!("{ROOT}/jobs{suffix}"), None)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn partial_transfer_holds_capacity_and_ranges_and_expiry_are_explicit(pool: PgPool) {
    let app = setup(&pool).await;
    let (id, _) = create(&app).await;
    let path = format!("{ROOT}/jobs/{id}/download");
    let mut ranged = request(Method::GET, &path, None);
    ranged
        .headers_mut()
        .insert("range", "bytes=0-1".parse().unwrap());
    assert_eq!(
        app.clone().oneshot(ranged).await.unwrap().status(),
        StatusCode::RANGE_NOT_SATISFIABLE
    );
    let response = app
        .clone()
        .oneshot(request(Method::GET, &path, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    drop(response);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.work_reservations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    sqlx::query(
        "UPDATE history.work_reservations SET expires_at=clock_timestamp()-interval '1 second'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let budgets = fleetiq_postgres::PostgresHistoryBudgets::new(pool.clone());
    let tenant = fleetiq_domain::TenantId::new("tenant-a").unwrap();
    let fleetiq_history::lifecycle::BudgetAdmission::Reserved(res) =
        budgets.reserve(&tenant, 2_097_152).await.unwrap()
    else {
        panic!("reserve")
    };
    budgets.release(&tenant, res).await.unwrap();
    sqlx::query("UPDATE history.export_jobs SET expires_at=clock_timestamp()-interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        json_response(&app, Method::GET, &path, None).await.0,
        StatusCode::GONE
    );
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn browser_mutations_and_closed_requests_cannot_bypass_authority(pool: PgPool) {
    drop(setup(&pool).await);
    let browser_app = app(&pool, true);
    let path = format!("{ROOT}/requests/browser");
    assert_eq!(
        json_response(&browser_app, Method::PUT, &path, Some(body()))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let mut r = request(Method::PUT, &path, Some(body()));
    r.headers_mut()
        .insert("origin", "https://fleet.example".parse().unwrap());
    r.headers_mut()
        .insert("x-fleetiq-csrf", "1".parse().unwrap());
    let response = browser_app.clone().oneshot(r).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let b: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 32768).await.unwrap()).unwrap();
    let id = b["export_id"].as_str().unwrap();
    assert_eq!(
        json_response(
            &browser_app,
            Method::DELETE,
            &format!("{ROOT}/jobs/{id}"),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let app = app(&pool, false);
    let mut forged = body();
    forged["tenant_id"] = json!("tenant-b");
    for bad in [
        forged,
        json!({"asset_id":"asset-a","property_id":"electrical.voltage","property_version":1,"start_at_ms":"00","end_at_ms":"3000"}),
    ] {
        assert_eq!(
            json_response(
                &app,
                Method::PUT,
                &format!("{ROOT}/requests/invalid"),
                Some(bad)
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        json_response(&app, Method::GET, &format!("{ROOT}/jobs/01"), None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        json_response(
            &app,
            Method::GET,
            &format!("{ROOT}/jobs/{id}?unknown=yes"),
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let mut unauthenticated = request(Method::GET, &format!("{ROOT}/jobs/{id}/download"), None);
    unauthenticated.headers_mut().remove("authorization");
    assert_eq!(
        app.clone().oneshot(unauthenticated).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn global_transfer_limit_is_independent_of_tenant_capacity(pool: PgPool) {
    let app = setup(&pool).await;
    let (id, _) = create(&app).await;
    sqlx::query("INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES('tenant-a',2,'operator-a',7,30,20,67108864)").execute(&pool).await.unwrap();
    let mut held = Vec::new();
    for _ in 0..8 {
        let response = app
            .clone()
            .oneshot(request(
                Method::GET,
                &format!("{ROOT}/jobs/{id}/download"),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        held.push(response);
    }
    assert_eq!(
        json_response(
            &app,
            Method::GET,
            &format!("{ROOT}/jobs/{id}/download"),
            None
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    drop(held);
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn head_metadata_never_reads_a_file_or_reserves_transfer_capacity(pool: PgPool) {
    let app = setup(&pool).await;
    let (id, _) = create(&app).await;
    let response = app
        .clone()
        .oneshot(request(
            Method::HEAD,
            &format!("{ROOT}/jobs/{id}/download"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        to_bytes(response.into_body(), 2_097_152)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.work_reservations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn mutation_cannot_combine_telemetry_and_operations_from_different_actors(pool: PgPool) {
    use crate::protected_request::{ProtectedRequestError, RequestAuthenticator};
    use fleetiq_access::AuthenticatedActor;
    use std::sync::atomic::{AtomicUsize, Ordering};
    type Auth = CallerAuthenticationService<
        crate::bearer_authentication::ConfiguredBearerAuthenticator,
        PostgresActorIdentityResolver,
    >;
    struct ChangedIdentity {
        a: Auth,
        b: Auth,
        calls: AtomicUsize,
    }
    impl RequestAuthenticator for ChangedIdentity {
        async fn authenticate(
            &self,
            headers: &axum::http::HeaderMap,
        ) -> Result<AuthenticatedActor, ProtectedRequestError> {
            let auth = if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                &self.a
            } else {
                &self.b
            };
            RequestAuthenticator::authenticate(auth, headers).await
        }
    }
    fn identity(pool: &PgPool, subject: &str) -> Auth {
        let config = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
            .unwrap()
            .with_bearer_identity(TOKEN, "workload", subject)
            .unwrap();
        CallerAuthenticationService::new(
            crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
                config.bearer_identity().unwrap().clone(),
            ),
            PostgresActorIdentityResolver::new(pool.clone()),
        )
    }
    drop(setup(&pool).await);
    sqlx::raw_sql("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a' AND actor_id='operator-a' AND action='operations.manage'; INSERT INTO identity.actors VALUES('operator-b'); INSERT INTO identity.external_actor_identities VALUES('workload','operator-b','operator-b'); INSERT INTO identity.tenant_action_grants VALUES('tenant-a','operator-b','operations.manage')").execute(&pool).await.unwrap();
    let app = crate::export_routes::router(
        pool.clone(),
        ChangedIdentity {
            a: identity(&pool, "operator"),
            b: identity(&pool, "operator-b"),
            calls: AtomicUsize::new(0),
        },
        crate::mutation_guard::MutationGuard::Workload,
    );
    assert_eq!(
        json_response(
            &app,
            Method::PUT,
            &format!("{ROOT}/requests/mixed-actors"),
            Some(body())
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.export_jobs")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}
