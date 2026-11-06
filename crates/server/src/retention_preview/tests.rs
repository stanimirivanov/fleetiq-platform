//! Read-only operational visibility and stale-context rejection.
use crate::alerts::tests::setup;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;
pub(super) async fn read(app: &Router, suffix: &str, token: bool) -> (StatusCode, Value) {
    let mut request = Request::get(format!(
        "/api/v1/tenants/tenant-a/retention-preview{suffix}"
    ));
    if token {
        request = request.header(
            "authorization",
            "Bearer aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    if status == StatusCode::OK {
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    let bytes = to_bytes(response.into_body(), 262_144).await.unwrap();
    (
        status,
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    )
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn preview_requires_independent_authority_and_rechecks_context(pool: PgPool) {
    let app = setup(&pool).await;
    assert_eq!(
        read(&app, "?preview=invalid", false).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        read(&app, "?preview=invalid", true).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES ('tenant-a','operator-a','operations.read')").execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a' AND action<>'operations.read'").execute(&pool).await.unwrap();
    let (status, initial) = read(&app, "", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(initial["policy_revision"], "0");
    assert_eq!(initial["reclamation_supported"], false);
    assert_eq!(initial["estimated_reclaimable_bytes"], "0");
    assert_eq!(initial["candidates"].as_array().unwrap().len(), 2);
    let token = initial["preview_token"].as_str().unwrap();
    assert_eq!(
        read(&app, &format!("?preview={token}"), true).await.0,
        StatusCode::OK
    );
    sqlx::query(
        "INSERT INTO analytics.prepared_evaluations VALUES ('tenant-a','new-consumer','{}','{}')",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        read(&app, &format!("?preview={token}"), true).await.0,
        StatusCode::CONFLICT
    );
    let (_, current) = read(&app, "", true).await;
    assert!(
        current["candidates"][0]["active_consumers"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("calculator"))
    );
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,2,8388608)").execute(&pool).await.unwrap();
    assert_eq!(
        read(
            &app,
            &format!("?preview={}", current["preview_token"].as_str().unwrap()),
            true
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (_, fresh) = read(&app, "", true).await;
    let mut expired: Value =
        crate::cursor_codec::decode(fresh["preview_token"].as_str().unwrap()).unwrap();
    let observed = expired["observed"].as_i64().unwrap() - 120_000;
    expired["observed"] = observed.into();
    expired["expires"] = (observed + 60_000).into();
    let expired = crate::cursor_codec::encode(&expired).unwrap();
    assert_eq!(
        read(&app, &format!("?preview={expired}"), true).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        read(&app, "?preview=invalid", true).await.0,
        StatusCode::BAD_REQUEST
    );
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='operations.read'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        read(&app, &format!("?preview={token}"), true).await.0,
        StatusCode::FORBIDDEN
    );
}
