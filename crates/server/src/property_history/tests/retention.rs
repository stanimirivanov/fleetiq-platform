//! Reclaimed evidence is explicit through legacy reads and exact through selected restores.
use super::*;
#[path = "../../../tests/hot_retention/support.rs"]
mod support;
use fleetiq_history::{
    restore::{RestoreClaim, RestoreRepository, RestoredSegment},
    retention_work::{RetentionRepository, RetentionTick},
};
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn reclaimed_history_requires_explicit_restore_and_preserves_provenance(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (target, original, bytes) = support::selection(&pool).await?;
    let app = app(&pool);
    let token = crate::cursor_codec::encode(&cursor::Interpretation::from_id(original[0].id()))?;
    let root = format!("{ROOT}/history?start_at_ms=0&end_at_ms=3000&limit=100");
    let (status, before) = response(&app, &root).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(before["changes"].as_array().unwrap().len(), 2);
    support::quiesce(&pool).await?;
    let repo = fleetiq_postgres::PostgresHotRetention::new(pool.clone());
    repo.approve(target.tenant(), 1, "operator-a").await?;
    let segment = RestoredSegment::new(bytes, original.clone())?;
    assert_eq!(
        repo.reclaim(&target, &support::Source(&segment)).await?,
        RetentionTick::Reclaimed(2)
    );
    let (status, body) = response(&app, &root).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["type"], "urn:fleetiq:problem:archived-history");
    assert_eq!(
        response(&app, &format!("{ROOT}/interpretations/{token}"))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let selected = format!(
        "{root}&mode=hot_and_restored&manifest_id={}&restore_generation=1",
        target.manifest()
    );
    let exact = format!("{ROOT}/interpretations/{token}");
    let query = format!("manifest_id={}&restore_generation=1", target.manifest());
    let lookup = format!("{exact}?{query}");
    assert_eq!(response(&app, &lookup).await.0, StatusCode::NOT_FOUND);
    let (status, archived) = response(&app, &selected).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(archived["coverage"]["archive"], "archived");
    let restores = fleetiq_postgres::PostgresRestore::new(pool.clone());
    let ticket = restores
        .request(target.tenant(), target.manifest(), 0)
        .await?;
    let RestoreClaim::Claimed(lease) = restores.claim(target.tenant(), ticket).await? else {
        panic!("restore lease");
    };
    assert!(restores.complete(&lease, &segment).await?);
    let (status, exact_restored) = response(&app, &lookup).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        before["changes"]
            .as_array()
            .unwrap()
            .contains(&exact_restored)
    );
    assert_eq!(exact_restored["interpretation"], token);
    for bad in [
        format!("{exact}?manifest_id=1"),
        format!("{exact}?restore_generation=1"),
        format!("{exact}?{query}&restore_generation=1"),
        format!("{exact}?manifest_id=01&restore_generation=1"),
        format!("{exact}?manifest_id=1&restore_generation=0"),
    ] {
        assert_eq!(response(&app, &bad).await.0, StatusCode::BAD_REQUEST);
    }
    for absent in [
        lookup.replace("restore_generation=1", "restore_generation=2"),
        lookup.replace("/assets/asset-a/", "/assets/asset-b/"),
        lookup.replace("manifest_id=1", "manifest_id=2"),
    ] {
        assert_eq!(response(&app, &absent).await.0, StatusCode::NOT_FOUND);
    }
    let cross_tenant = lookup.replace("/tenants/tenant-a/", "/tenants/tenant-b/");
    assert_eq!(response(&app, &cross_tenant).await.0, StatusCode::FORBIDDEN);
    sqlx::query("INSERT INTO identity.tenant_action_grants VALUES('tenant-b','operator-a','telemetry.read')").execute(&pool).await?;
    assert_eq!(response(&app, &cross_tenant).await.0, StatusCode::NOT_FOUND);

    let (status, restored) = response(&app, &selected).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restored["coverage"]["archive"], "restored");
    // Collect bounded pages; the semantic worker legitimately added later evidence.
    let mut changes = restored["changes"].as_array().unwrap().clone();
    let mut next = restored["next_after"].as_str().map(str::to_owned);
    for _ in 0..4 {
        let Some(token) = next.take() else {
            break;
        };
        let (status, page) = response(&app, &format!("{selected}&after={token}")).await;
        assert_eq!(status, StatusCode::OK);
        changes.extend(page["changes"].as_array().unwrap().iter().cloned());
        next = page["next_after"].as_str().map(str::to_owned);
    }
    assert!(next.is_none());
    for expected in before["changes"].as_array().unwrap() {
        assert!(changes.contains(expected));
    }
    let hot = changes
        .iter()
        .find(|change| !before["changes"].as_array().unwrap().contains(change))
        .unwrap();
    let hot_lookup = format!(
        "{ROOT}/interpretations/{}",
        hot["interpretation"].as_str().unwrap()
    );
    assert_eq!(response(&app, &hot_lookup).await.0, StatusCode::OK);
    assert_eq!(
        response(&app, &format!("{hot_lookup}?{query}")).await.0,
        StatusCode::NOT_FOUND
    );
    sqlx::query(
        "UPDATE history.restore_jobs SET expires_at=clock_timestamp() WHERE tenant_id='tenant-a'",
    )
    .execute(&pool)
    .await?;
    assert_eq!(response(&app, &lookup).await.0, StatusCode::NOT_FOUND);
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='telemetry.read'")
        .execute(&pool)
        .await?;
    assert_eq!(response(&app, &root).await.0, StatusCode::FORBIDDEN);
    assert_eq!(response(&app, &lookup).await.0, StatusCode::FORBIDDEN);
    Ok(())
}
