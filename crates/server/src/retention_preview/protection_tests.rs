use super::tests::read;
use crate::alerts::tests::setup;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use fleetiq_domain::TenantId;
use fleetiq_history::archive_work::{ArchiveClaim, ArchiveObject, ArchiveWorkRepository};
use fleetiq_postgres::{PostgresArchiveWork, PostgresRestore};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn publication_floor_and_live_restore_remain_protected(pool: PgPool) {
    let app = setup(&pool).await;
    sqlx::raw_sql("INSERT INTO identity.tenant_action_grants VALUES ('tenant-a','operator-a','operations.read'); INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,2,8388608)").execute(&pool).await.unwrap();
    let tenant = TenantId::new("tenant-a").unwrap();
    let archive = PostgresArchiveWork::new(pool.clone());
    let ArchiveClaim::Claimed(lease) = archive.claim(&tenant).await.unwrap() else {
        panic!("selection")
    };
    let (_, unverified) = read(&app, "", true).await;
    assert_eq!(unverified["candidates"][0]["archive_verified"], false);
    let bytes = fleetiq_archive::encode(&tenant, &lease.changes).unwrap();
    let object = ArchiveObject::new(
        Sha256::digest(&bytes).into(),
        i64::try_from(bytes.len()).unwrap(),
    )
    .unwrap();
    assert!(archive.complete(&lease, object).await.unwrap());
    let (_, published) = read(&app, "", true).await;
    assert_eq!(published["candidates"][0]["archive_verified"], true);
    assert!(
        published["candidates"][0]["age_floor_ms"]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .unwrap()
            > published["observed_at_ms"]
                .as_str()
                .unwrap()
                .parse::<i64>()
                .unwrap()
    );
    assert!(
        published["candidates"][0]["protections"]
            .as_array()
            .unwrap()
            .contains(&json!("hot_retention"))
    );
    PostgresRestore::new(pool.clone())
        .request(&tenant, lease.job_id, 0)
        .await
        .unwrap();
    assert_eq!(
        read(
            &app,
            &format!("?preview={}", published["preview_token"].as_str().unwrap()),
            true
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (_, restoring) = read(&app, "", true).await;
    assert!(
        restoring["candidates"][0]["active_consumers"]
            .as_array()
            .unwrap()
            .contains(&json!("restore"))
    );
    assert_eq!(restoring["estimated_reclaimable_bytes"], "0");
    let spec: Value =
        serde_json::from_str(include_str!("../../../../contracts/http/dist/openapi.json")).unwrap();
    let schema =
        json!({"components":spec["components"],"$ref":"#/components/schemas/RetentionPreview"});
    assert!(
        jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(&restoring)
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn previews_are_bounded_tenant_scoped_and_do_not_mutate_evidence(pool: PgPool) {
    let app = setup(&pool).await;
    sqlx::raw_sql("INSERT INTO identity.tenant_action_grants VALUES ('tenant-a','operator-a','operations.read'),('tenant-b','operator-a','operations.read'); INSERT INTO history.asset_property_changes SELECT tenant_id,message_id,protocol_package_id,protocol_package_version,n,'binding-1',1,'device_recorded',CASE WHEN n=1 THEN 500 ELSE 1000 END,1000,150 FROM ingestion.accepted_messages CROSS JOIN generate_series(1,106) n").execute(&pool).await.unwrap();
    let (_, a) = read(&app, "", true).await;
    assert_eq!(a["candidates"].as_array().unwrap().len(), 100);
    assert_eq!(a["has_more"], true);
    let token = a["preview_token"].as_str().unwrap();
    let request = Request::get("/api/v1/tenants/tenant-b/retention-preview")
        .header(
            "authorization",
            "Bearer aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let b: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 262_144).await.unwrap()).unwrap();
    assert_eq!(b["candidates"], json!([]));
    let request = Request::get(format!(
        "/api/v1/tenants/tenant-b/retention-preview?preview={token}"
    ))
    .header(
        "authorization",
        "Bearer aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .body(Body::empty())
    .unwrap();
    assert_eq!(
        app.oneshot(request).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM history.asset_property_changes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 108);
}
use tower::ServiceExt;
