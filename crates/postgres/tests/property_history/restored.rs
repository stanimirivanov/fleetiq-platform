//! Exact union and continuation include restored-only interpretations without changing DB ordering.
use super::*;
use fleetiq_history::{DecodedObservationView, RestoredPropertyHistoryView, StoredPropertyChange};
use fleetiq_postgres::PostgresDecodedObservationView;
use fleetiq_semantics::{AssetPropertyChangeId, EventTimeBasis};
use fleetiq_telemetry::DecodedObservationId;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn restored_only_rows_and_overlap_paginate_without_skips(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool).await?;
    let tenant = TenantId::new("tenant-a")?;
    let view = PostgresAssetPropertyChangeView::new(pool.clone());
    let hot = view
        .list_property(&tenant, &query(0, 3000, 100, None))
        .await?;
    let existing = &hot.changes()[0];
    let source = existing.id().observation_id();
    let source =
        DecodedObservationId::new(source.message_id().clone(), source.decoded_by().clone(), 1);
    let observation = PostgresDecodedObservationView::new(pool)
        .find(&tenant, &source)
        .await?
        .unwrap();
    let id = AssetPropertyChangeId::from_parts(
        source,
        existing.id().binding_id().clone(),
        existing.id().binding_revision(),
        EventTimeBasis::DeviceRecorded,
    );
    let extra = StoredPropertyChange::new(
        id,
        observation,
        existing.target().clone(),
        UnixTimeMillis::new(500),
        existing.first_resolution(),
    )?;
    let restored = vec![existing.clone(), extra.clone()];
    let all = view
        .list_with_restored(&tenant, &query(0, 3000, 100, None), &restored)
        .await?;
    assert_eq!(
        all.changes().len(),
        3,
        "overlap must not duplicate interpretations"
    );
    let first = view
        .list_with_restored(&tenant, &query(0, 3000, 1, None), &restored)
        .await?;
    assert_eq!(first.changes(), [extra]);
    let second = view
        .list_with_restored(
            &tenant,
            &query(0, 3000, 1, first.next_after().cloned()),
            &restored,
        )
        .await?;
    assert_eq!(second.changes(), std::slice::from_ref(existing));
    let third = view
        .list_with_restored(
            &tenant,
            &query(0, 3000, 1, second.next_after().cloned()),
            &restored,
        )
        .await?;
    assert_eq!(third.changes(), [hot.changes()[1].clone()]);
    assert!(third.next_after().is_none());
    assert!(
        view.list_with_restored(
            &TenantId::new("tenant-b")?,
            &query(0, 3000, 1, None),
            &restored
        )
        .await
        .is_err()
    );
    Ok(())
}
