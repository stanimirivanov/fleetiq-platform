//! HTTP coverage, exact overlap and cursor/authority boundaries with real PostgreSQL.
use super::*;
use fleetiq_domain::TenantId;
use fleetiq_history::{
    archive_work::{ArchiveClaim, ArchiveObject, ArchiveWorkRepository},
    restore::{RestoreClaim, RestoreRepository, RestoredSegment},
};
use fleetiq_postgres::{PostgresArchiveWork, PostgresRestore};
use sha2::{Digest, Sha256};
pub(super) async fn publish(pool: &PgPool) -> (i64, RestoredSegment) {
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,2,8388608)").execute(pool).await.unwrap();
    let tenant = TenantId::new("tenant-a").unwrap();
    let archive = PostgresArchiveWork::new(pool.clone());
    let ArchiveClaim::Claimed(lease) = archive.claim(&tenant).await.unwrap() else {
        panic!("selection")
    };
    let bytes = fleetiq_archive::encode(&tenant, &lease.changes).unwrap();
    let object = ArchiveObject::new(
        Sha256::digest(&bytes).into(),
        i64::try_from(bytes.len()).unwrap(),
    )
    .unwrap();
    assert!(archive.complete(&lease, object).await.unwrap());
    (
        lease.job_id,
        RestoredSegment::new(bytes, lease.changes).unwrap(),
    )
}
fn url(manifest: i64, generation: i64) -> String {
    format!(
        "{ROOT}/history?start_at_ms=0&end_at_ms=3000&limit=1&mode=hot_and_restored&manifest_id={manifest}&restore_generation={generation}"
    )
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn coverage_and_restore_scoped_pages_remain_explicit(pool: PgPool) {
    let app = setup(&pool).await;
    let (manifest, segment) = publish(&pool).await;
    let first_url = url(manifest, 1);
    let (status, archived) = response(&app, &first_url).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(archived["coverage"]["archive"], "archived");
    let tenant = TenantId::new("tenant-a").unwrap();
    let repo = PostgresRestore::new(pool.clone());
    let ticket = repo.request(&tenant, manifest, 0).await.unwrap();
    let (_, pending) = response(&app, &first_url).await;
    assert_eq!(pending["coverage"]["archive"], "restoring");
    let RestoreClaim::Claimed(lease) = repo.claim(&tenant, ticket).await.unwrap() else {
        panic!("claim")
    };
    assert!(repo.complete(&lease, &segment).await.unwrap());
    assert_eq!(
        response(
            &app,
            &format!(
                "{first_url}&after={}",
                pending["next_after"].as_str().unwrap()
            )
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, first) = response(&app, &first_url).await;
    assert_eq!(first["coverage"]["archive"], "restored");
    assert_eq!(first["coverage"]["scope"], "selected_manifest_only");
    assert_eq!(first["changes"].as_array().unwrap().len(), 1);
    assert_eq!(
        first["changes"][0]["value"],
        json!({"kind":"integer","value":"9223372036854775807"})
    );
    let after = first["next_after"].as_str().unwrap();
    let (_, second) = response(&app, &format!("{first_url}&after={after}")).await;
    assert_eq!(second["changes"].as_array().unwrap().len(), 1);
    assert_ne!(
        first["changes"][0]["interpretation"],
        second["changes"][0]["interpretation"]
    );
    assert!(second["next_after"].is_null());
    assert_eq!(
        response(&app, &format!("{}&after={after}", url(manifest, 2)))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        response(
            &app,
            &format!("{ROOT}/history?start_at_ms=0&end_at_ms=3000&after={after}")
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    sqlx::query("UPDATE history.restore_jobs SET expires_at=clock_timestamp()-interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        response(&app, &format!("{first_url}&after={after}"))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (_, expired) = response(&app, &first_url).await;
    assert_eq!(expired["coverage"]["archive"], "unavailable");
    assert!(expired["next_after"].is_null());
    for body in [&archived, &pending, &first, &expired] {
        assert_contract(body);
    }
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        response(&app, &format!("{first_url}&after=invalid"))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

fn assert_contract(body: &Value) {
    let contract: Value = serde_json::from_str(include_str!(
        "../../../../../contracts/http/dist/openapi.json"
    ))
    .unwrap();
    let schema = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","components":{"schemas":contract["components"]["schemas"]},"$ref":"#/components/schemas/PropertyHistoryPage"});
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(body));
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn invalid_ready_file_is_unavailable_not_complete_empty_history(pool: PgPool) {
    let app = setup(&pool).await;
    let tenant = TenantId::new("tenant-a").unwrap();
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,2,8388608)").execute(&pool).await.unwrap();
    let archive = PostgresArchiveWork::new(pool.clone());
    let ArchiveClaim::Claimed(lease) = archive.claim(&tenant).await.unwrap() else {
        panic!("selection")
    };
    // Simulate a faulty trusted writer: integrity metadata agrees but bytes are not a valid codec file.
    let bytes = b"not-a-supported-archive".to_vec();
    let object = ArchiveObject::new(
        Sha256::digest(&bytes).into(),
        i64::try_from(bytes.len()).unwrap(),
    )
    .unwrap();
    assert!(archive.complete(&lease, object).await.unwrap());
    let repo = PostgresRestore::new(pool.clone());
    let ticket = repo.request(&tenant, lease.job_id, 0).await.unwrap();
    let RestoreClaim::Claimed(claim) = repo.claim(&tenant, ticket).await.unwrap() else {
        panic!("claim")
    };
    assert!(
        repo.complete(&claim, &RestoredSegment::new(bytes, lease.changes).unwrap())
            .await
            .unwrap()
    );
    let (status, body) = response(&app, &url(lease.job_id, 1)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["coverage"]["archive"], "unavailable");
    assert_eq!(body["changes"].as_array().unwrap().len(), 1);
    assert!(body["next_after"].is_null());
    assert_contract(&body);
    let (_, hot) = response(
        &app,
        &format!("{ROOT}/history?start_at_ms=0&end_at_ms=3000"),
    )
    .await;
    assert!(hot.get("coverage").is_none());
}
