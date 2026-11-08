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
    let opted = format!("{selected}&include_restore_membership=true");
    assert_eq!(
        response(&app, &format!("{root}&include_restore_membership=true"))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (status, archived_membership) = response(&app, &opted).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        archived_membership["restored_row_indices"],
        serde_json::json!([])
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
    assert!(restored.get("restored_row_indices").is_none());
    let (status, mixed) = response(&app, &opted).await;
    assert_eq!(status, StatusCode::OK);
    let members = before["changes"].as_array().unwrap();
    let expected_indices: Vec<_> = mixed["changes"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .filter_map(|(index, row)| members.contains(row).then_some(index))
        .collect();
    assert_eq!(mixed["changes"], restored["changes"]);
    assert_eq!(mixed["next_after"], restored["next_after"]);
    assert_eq!(
        mixed["restored_row_indices"],
        serde_json::json!(expected_indices)
    );
    assert_eq!(
        response(
            &app,
            &format!("{selected}&include_restore_membership=false")
        )
        .await
        .1,
        restored
    );
    let paged = opted.replace("limit=100", "limit=1");
    let mut url = paged.clone();
    for (index, expected) in changes.iter().enumerate() {
        let (status, page) = response(&app, &url).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(page["changes"], serde_json::json!([expected]));
        let indices = if members.contains(expected) {
            serde_json::json!([0])
        } else {
            serde_json::json!([])
        };
        assert_eq!(page["restored_row_indices"], indices);
        if let Some(after) = page["next_after"].as_str() {
            assert!(index + 1 < changes.len());
            url = format!("{paged}&after={after}");
        } else {
            assert_eq!(index + 1, changes.len());
        }
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
    let (status, expired) = response(&app, &opted).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(expired["restored_row_indices"], serde_json::json!([]));
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE action='telemetry.read'")
        .execute(&pool)
        .await?;
    assert_eq!(response(&app, &root).await.0, StatusCode::FORBIDDEN);
    assert_eq!(response(&app, &lookup).await.0, StatusCode::FORBIDDEN);
    Ok(())
}
