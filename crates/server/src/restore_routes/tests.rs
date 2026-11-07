//! Protected restore requests against the real durable and object boundaries.
#[path = "../../../archive/tests/support/provider.rs"]
mod provider;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use fleetiq_access::CallerAuthenticationService;
use fleetiq_archive::objects::{ImmutableArchiveObjects, S3ArchiveConfig};
use fleetiq_domain::TenantId;
use fleetiq_history::archive_work::{ArchiveClaim, ArchiveSegmentStore, ArchiveWorkRepository};
use fleetiq_postgres::{PostgresActorIdentityResolver, PostgresArchiveWork};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROOT: &str = "/api/v1/tenants/tenant-a/restores/manifests";

fn app(pool: &PgPool, archive: Option<S3ArchiveConfig>) -> Router {
    let config = crate::ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
        .unwrap()
        .with_bearer_identity(TOKEN, "workload", "operator")
        .unwrap();
    crate::restore_routes::router(
        pool.clone(),
        CallerAuthenticationService::new(
            crate::bearer_authentication::ConfiguredBearerAuthenticator::new(
                config.bearer_identity().unwrap().clone(),
            ),
            PostgresActorIdentityResolver::new(pool.clone()),
        ),
        crate::mutation_guard::MutationGuard::Browser {
            public_origin: "https://fleet.example".into(),
        },
        archive,
    )
}

async fn send(app: &Router, method: Method, path: &str, marker: bool) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {TOKEN}"));
    if marker {
        request = request
            .header("origin", "https://fleet.example")
            .header("x-fleetiq-csrf", "1");
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    (
        response.status(),
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap(),
    )
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn authorized_restore_replays_and_reports_exact_availability(pool: PgPool) {
    sqlx::raw_sql(include_str!(
        "../../../postgres/tests/property_history/topology.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES('tenant-a',1,'operator-a',7,30,4,16777216)")
        .execute(&pool).await.unwrap();
    let fixture = provider::Provider::start().await;
    let tenant = TenantId::new("tenant-a").unwrap();
    let archive = PostgresArchiveWork::new(pool.clone());
    let ArchiveClaim::Claimed(selection) = archive.claim(&tenant).await.unwrap() else {
        panic!("archive selection");
    };
    let objects = ImmutableArchiveObjects::connect(fixture.config()).unwrap();
    let object = objects.store(&tenant, &selection.changes).await.unwrap();
    assert!(archive.complete(&selection, object).await.unwrap());
    let manifest = selection.job_id;
    let put = format!("{ROOT}/{manifest}/requests/0");
    let get = format!("{ROOT}/{manifest}/generations/1");
    let protected = app(&pool, Some(fixture.config()));
    assert_eq!(
        send(&protected, Method::PUT, &put, true).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES('tenant-a','operator-a','operations.manage')")
        .execute(&pool).await.unwrap();
    assert_eq!(
        send(&protected, Method::PUT, &put, false).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&app(&pool, None), Method::PUT, &put, true).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        send(&protected, Method::PUT, &format!("{put}?extra=1"), true)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(&protected, Method::GET, &get, false).await.0,
        StatusCode::NOT_FOUND
    );
    fixture.pause();
    let (status, pending) = send(&protected, Method::PUT, &put, true).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(pending["manifest_id"], manifest.to_string());
    assert_eq!(pending["restore_generation"], "1");
    assert_eq!(pending["state"], "pending");
    assert_eq!(
        send(&protected, Method::PUT, &put, true).await,
        (StatusCode::ACCEPTED, pending)
    );
    fixture.resume();
    let ready = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let (status, receipt) = send(&protected, Method::GET, &get, false).await;
            assert_eq!(status, StatusCode::OK);
            if receipt["state"] == "ready" {
                break receipt;
            }
            assert_eq!(receipt["state"], "pending");
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        send(&protected, Method::PUT, &put, true).await,
        (StatusCode::ACCEPTED, ready.clone())
    );
    crate::contract_test_support::assert_published_example(
        &serde_json::to_vec(&ready).unwrap(),
        "/api/v1/tenants/{tenant_id}/restores/manifests/{manifest_id}/generations/{restore_generation}",
    );
    let newer = format!("{ROOT}/{manifest}/requests/1");
    assert_eq!(
        send(&protected, Method::PUT, &newer, true).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(
            &protected,
            Method::GET,
            &get.replace("tenant-a", "tenant-b"),
            false
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE history.restore_jobs SET expires_at=clock_timestamp()-interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    let (status, expired) = send(&protected, Method::PUT, &put, true).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(expired["state"], "expired");
    assert_eq!(expired["restore_generation"], "1");
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a' AND action='telemetry.read'")
        .execute(&pool).await.unwrap();
    assert_eq!(
        send(&protected, Method::GET, &get, false).await.0,
        StatusCode::FORBIDDEN
    );
    fixture.close();
}
