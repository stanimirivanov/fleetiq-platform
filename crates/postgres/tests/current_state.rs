//! Real PostgreSQL queue, revision, replay and snapshot boundaries.
#[path = "current_state/support.rs"]
mod support;
use fleetiq_domain::TenantId;
use fleetiq_postgres::PostgresCurrentStateRepository;
use fleetiq_projection::{
    CurrentStateRepository, ProjectionMaintenance, ProjectionTick, compare_precedence,
};
use sqlx::PgPool;
use std::error::Error;
use support::*;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn late_data_duplicates_and_rebuild_converge_with_exact_provenance(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool).await?;
    let repository = PostgresCurrentStateRepository::new(pool.clone());
    let tenant = TenantId::new("tenant-a")?;
    let before = repository
        .snapshot(&tenant, &target("electrical.voltage", 1)?)
        .await?
        .unwrap();
    assert_eq!(before.state().items(), []);
    assert!(before.pending());
    assert_eq!(before.state().revision().get(), 0);
    assert_eq!(repository.tick().await?, ProjectionTick::Applied(2));
    let first = repository
        .snapshot(&tenant, &target("electrical.voltage", 1)?)
        .await?
        .unwrap();
    assert_eq!(first.state().items()[0].event_at().get(), 2000);
    assert!(!first.pending());
    append(&pool, 1, "device_recorded", 500).await?;
    repository.tick().await?;
    let late = repository
        .snapshot(&tenant, &target("electrical.voltage", 1)?)
        .await?
        .unwrap();
    assert_eq!(first.state().items(), late.state().items());
    append(&pool, 106, "received_at", 2000).await?;
    repository.tick().await?;
    let last = repository
        .snapshot(&tenant, &target("electrical.voltage", 1)?)
        .await?
        .unwrap();
    assert_eq!(
        last.state().items()[0].observation().id().output_index(),
        106
    );
    assert_eq!(
        compare_precedence(&last.state().items()[0], &first.state().items()[0]),
        std::cmp::Ordering::Greater
    );
    assert_eq!(last.processed_count(), 4);
    append(&pool, 106, "received_at", 2000).await?;
    assert_eq!(repository.tick().await?, ProjectionTick::Idle);
    assert_eq!(
        last,
        repository
            .snapshot(&tenant, &target("electrical.voltage", 1)?)
            .await?
            .unwrap()
    );
    repository.rebuild(&tenant).await?;
    let rebuilt = repository
        .snapshot(&tenant, &target("electrical.voltage", 1)?)
        .await?
        .unwrap();
    assert_eq!(rebuilt.state().items(), last.state().items());
    assert_eq!(rebuilt.processed_count(), 4);
    assert_eq!(rebuilt.generation(), last.generation() + 1);
    assert_eq!(
        rebuilt.state().revision().get(),
        last.state().revision().get() + 1
    );
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn an_older_queue_id_committing_after_a_batch_is_not_lost(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool).await?;
    let mut delayed = pool.begin().await?;
    sqlx::query(INSERT)
        .bind(1_i64)
        .bind("device_recorded")
        .bind(500_i64)
        .execute(&mut *delayed)
        .await?;
    append(&pool, 2, "device_recorded", 1000).await?;
    let repository = PostgresCurrentStateRepository::new(pool.clone());
    assert_eq!(repository.tick().await?, ProjectionTick::Applied(3));
    delayed.commit().await?;
    assert_eq!(repository.tick().await?, ProjectionTick::Applied(1));
    let s = repository
        .snapshot(
            &TenantId::new("tenant-a")?,
            &target("electrical.voltage", 1)?,
        )
        .await?
        .unwrap();
    assert_eq!(s.processed_count(), 4);
    assert!(!s.pending());
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_batches_are_bounded_and_rollback_keeps_queue_revision_and_values_atomic(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool).await?;
    let mut tx = pool.begin().await?;
    let count: i64 = sqlx::query_scalar("SELECT applied_count FROM projection.project_next()")
        .fetch_one(&mut *tx)
        .await?;
    assert_eq!(count, 2);
    tx.rollback().await?;
    let repository = PostgresCurrentStateRepository::new(pool.clone());
    let s = repository
        .snapshot(
            &TenantId::new("tenant-a")?,
            &target("electrical.voltage", 1)?,
        )
        .await?
        .unwrap();
    assert_eq!(s.state().revision().get(), 0);
    assert_eq!(s.state().items(), []);
    assert!(s.pending());
    sqlx::query("INSERT INTO history.asset_property_changes (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis,event_at_ms,resolution_effective_at_ms,resolution_known_at_ms) SELECT 'tenant-a','message-1','vendor.protocol','1',n,'binding-1',1,'received_at',2000,1000,150 FROM generate_series(0,106) n ON CONFLICT DO NOTHING").execute(&pool).await?;
    let a = repository.clone();
    let b = repository.clone();
    let left = tokio::spawn(async move { a.tick().await });
    let right = tokio::spawn(async move { b.tick().await });
    for outcome in [left.await??, right.await??] {
        if let ProjectionTick::Applied(n) = outcome {
            assert!((1..=100).contains(&n));
        }
    }
    drain(&repository).await?;
    let s = repository
        .snapshot(
            &TenantId::new("tenant-a")?,
            &target("electrical.voltage", 1)?,
        )
        .await?
        .unwrap();
    assert_eq!(s.processed_count(), 108);
    assert_eq!(s.state().revision().get(), 2);
    assert!(!s.pending());
    assert_eq!(s.state().items()[0].observation().id().output_index(), 106);
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn corrected_targets_do_not_erase_original_interpretations_and_tenant_scope_is_closed(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool).await?;
    let repository = PostgresCurrentStateRepository::new(pool.clone());
    repository.tick().await?;
    sqlx::raw_sql("UPDATE metadata.signal_bindings SET latest_revision=2 WHERE tenant_id='tenant-a' AND binding_id='binding-1';
        INSERT INTO metadata.signal_binding_revisions (tenant_id,binding_id,revision,recorded_at_ms,source_device_asset_id,source_endpoint_id,source_signal_id,target_asset_id,target_property_id,target_property_version,effective_start_ms,effective_end_ms) VALUES ('tenant-a','binding-1',2,200,'gateway-1','can-primary','bus.voltage','asset-b','electrical.voltage',1,0,NULL);
        INSERT INTO history.asset_property_changes (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis,event_at_ms,resolution_effective_at_ms,resolution_known_at_ms) VALUES ('tenant-a','message-1','vendor.protocol','1',0,'binding-1',2,'device_recorded',1000,1000,250);").execute(&pool).await?;
    repository.tick().await?;
    let tenant = TenantId::new("tenant-a")?;
    let old = repository
        .snapshot(&tenant, &target("electrical.voltage", 1)?)
        .await?
        .unwrap();
    assert_eq!(old.state().items()[0].event_at().get(), 2000);
    let mut b = target("electrical.voltage", 1)?;
    b = fleetiq_domain::SignalTarget::new(
        fleetiq_domain::AssetId::new("asset-b")?,
        b.property().clone(),
    );
    let corrected = repository.snapshot(&tenant, &b).await?.unwrap();
    assert_eq!(
        corrected.state().items()[0].id().binding_revision().get(),
        2
    );
    assert!(
        repository
            .snapshot(&TenantId::new("tenant-b")?, &b)
            .await?
            .is_none()
    );
    assert!(
        repository
            .snapshot(&tenant, &target("electrical.voltage", 2)?)
            .await?
            .is_none()
    );
    append(&pool, 2, "device_recorded", 1000).await?;
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await?;
    assert_eq!(repository.tick().await?, ProjectionTick::Idle);
    assert!(repository.rebuild(&tenant).await.is_err());
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn rebuild_retains_later_commits_and_rejects_wrong_isolation_and_corrupt_lineage(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool).await?;
    assert!(
        sqlx::query("SELECT projection.rebuild_current('tenant-a')")
            .execute(&pool)
            .await
            .is_err()
    );
    let mut rebuild = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *rebuild)
        .await?;
    let _: i64 = sqlx::query_scalar("SELECT count(*) FROM history.asset_property_changes")
        .fetch_one(&mut *rebuild)
        .await?;
    append(&pool, 1, "device_recorded", 500).await?;
    sqlx::query("SELECT projection.rebuild_current('tenant-a')")
        .execute(&mut *rebuild)
        .await?;
    rebuild.commit().await?;
    let repository = PostgresCurrentStateRepository::new(pool.clone());
    let tenant = TenantId::new("tenant-a")?;
    let s = repository
        .snapshot(&tenant, &target("electrical.voltage", 1)?)
        .await?
        .unwrap();
    assert!(s.pending());
    assert_eq!(s.processed_count(), 2);
    assert_eq!(repository.tick().await?, ProjectionTick::Applied(1));
    assert!(
        sqlx::query(
            "UPDATE projection.current_properties SET event_at_ms=0 WHERE tenant_id='tenant-a'"
        )
        .execute(&pool)
        .await
        .is_err()
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn latest_invalid_quality_is_visible_and_decimal_precision_is_exact(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool).await?;
    sqlx::raw_sql("INSERT INTO ingestion.accepted_messages (tenant_id,message_id,device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload) VALUES ('tenant-a','new-values','gateway-1','vendor.protocol','1',3000,'neutral'::bytea);
        INSERT INTO ingestion.decoded_message_results (tenant_id,message_id,protocol_package_id,protocol_package_version,observation_count) VALUES ('tenant-a','new-values','vendor.protocol','1',2);
        INSERT INTO ingestion.decoded_observations (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,endpoint_id,signal_id,device_recorded_at_ms,value_kind,value_integer,source_unit,quality) VALUES ('tenant-a','new-values','vendor.protocol','1',0,'can-primary','bus.voltage',3000,'integer',9,'volt','invalid');
        INSERT INTO ingestion.decoded_observations (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,endpoint_id,signal_id,device_recorded_at_ms,value_kind,value_decimal,quality) VALUES ('tenant-a','new-values','vendor.protocol','1',1,'can-primary','exact',3000,'decimal',12345678901234567890.12345678901234567890123456789,'uncertain');
        INSERT INTO history.asset_property_changes (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis,event_at_ms,resolution_effective_at_ms,resolution_known_at_ms) VALUES ('tenant-a','new-values','vendor.protocol','1',0,'binding-1',1,'device_recorded',3000,3000,150),('tenant-a','new-values','vendor.protocol','1',1,'binding-decimal',1,'device_recorded',3000,3000,150);").execute(&pool).await?;
    let repository = PostgresCurrentStateRepository::new(pool);
    drain(&repository).await?;
    let tenant = TenantId::new("tenant-a")?;
    let voltage = repository
        .snapshot(&tenant, &target("electrical.voltage", 1)?)
        .await?
        .unwrap();
    assert_eq!(
        voltage.state().items()[0].observation().quality(),
        fleetiq_telemetry::ObservationQuality::Invalid
    );
    let decimal = repository
        .snapshot(&tenant, &target("exact.decimal", 1)?)
        .await?
        .unwrap();
    let fleetiq_domain::PropertyValue::Decimal(value) =
        decimal.state().items()[0].observation().value()
    else {
        panic!("expected exact decimal");
    };
    let expected: bigdecimal::BigDecimal =
        "12345678901234567890.12345678901234567890123456789".parse()?;
    assert_eq!(value, &expected);
    assert_eq!(
        decimal.state().items()[0].observation().quality(),
        fleetiq_telemetry::ObservationQuality::Uncertain
    );
    Ok(())
}
#[sqlx::test(migrations = false)]
async fn upgrade_backfills_existing_interpretations_and_enqueues_new_ones(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    // Exercise real predecessor DDL, populate history before the new trigger,
    // then apply the additive migration transactionally. Generic schema tests
    // separately verify the exact SQLx ledger/checksum inventory.
    for migration in fleetiq_postgres::MIGRATOR.iter().filter(|m| m.version < 37) {
        sqlx::raw_sql(migration.sql.clone()).execute(&pool).await?;
    }
    setup(&pool).await?;
    let mut tx = pool.begin().await?;
    sqlx::raw_sql(include_str!(
        "../../../migrations/0037_current_property_projection.sql"
    ))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let repository = PostgresCurrentStateRepository::new(pool.clone());
    assert_eq!(repository.tick().await?, ProjectionTick::Applied(2));
    append(&pool, 1, "device_recorded", 500).await?;
    assert_eq!(repository.tick().await?, ProjectionTick::Applied(1));
    Ok(())
}
