//! Real PostgreSQL evidence for bounded history; no fixtures disable lineage.
use fleetiq_domain::{
    AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, SignalTarget,
    TenantId, UnixTimeMillis,
};
use fleetiq_history::{PropertyHistoryCursor, PropertyHistoryQuery, PropertyHistoryView};
use fleetiq_postgres::PostgresAssetPropertyChangeView;
use sqlx::PgPool;
use std::error::Error;

async fn setup(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(include_str!("property_history/topology.sql"))
        .execute(pool)
        .await?;
    Ok(())
}
fn query(
    start: i64,
    end: i64,
    limit: u8,
    after: Option<PropertyHistoryCursor>,
) -> PropertyHistoryQuery {
    PropertyHistoryQuery::new(
        SignalTarget::new(
            AssetId::new("asset-a").unwrap(),
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new("electrical.voltage").unwrap(),
                DefinitionVersion::new(1).unwrap(),
            ),
        ),
        UnixTimeMillis::new(start),
        UnixTimeMillis::new(end),
        limit,
        after,
    )
    .unwrap()
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn history_has_exclusive_bounds_ties_precision_and_tenant_isolation(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool).await?;
    let view = PostgresAssetPropertyChangeView::new(pool.clone());
    let tenant = TenantId::new("tenant-a")?;
    let first = view
        .list_property(&tenant, &query(0, 3000, 1, None))
        .await?;
    assert_eq!(first.changes().len(), 1);
    assert_eq!(first.changes()[0].event_at().get(), 1000);
    assert_eq!(
        first.changes()[0].observation().value(),
        &fleetiq_domain::PropertyValue::Integer(i64::MAX)
    );
    let second = view
        .list_property(&tenant, &query(0, 3000, 1, first.next_after().cloned()))
        .await?;
    assert_eq!(second.changes()[0].event_at().get(), 2000);
    assert!(second.next_after().is_none());
    assert_eq!(
        view.list_property(&tenant, &query(1000, 2000, 100, None))
            .await?
            .changes()
            .len(),
        1
    );
    assert!(
        view.list_property(&TenantId::new("tenant-b")?, &query(0, 3000, 100, None))
            .await?
            .changes()
            .is_empty()
    );
    // A second binding of the same output at the same event time remains distinct.
    sqlx::raw_sql("INSERT INTO metadata.signal_bindings (tenant_id,binding_id,latest_revision) VALUES ('tenant-a','binding-2',1); INSERT INTO metadata.signal_binding_revisions (tenant_id,binding_id,revision,recorded_at_ms,source_device_asset_id,source_endpoint_id,source_signal_id,target_asset_id,target_property_id,target_property_version,effective_start_ms,effective_end_ms) SELECT tenant_id,'binding-2',revision,recorded_at_ms,source_device_asset_id,source_endpoint_id,source_signal_id,target_asset_id,target_property_id,target_property_version,effective_start_ms,effective_end_ms FROM metadata.signal_binding_revisions WHERE binding_id='binding-1';").execute(&pool).await?;
    sqlx::query("INSERT INTO history.asset_property_changes SELECT tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,'binding-2',binding_revision,event_time_basis,event_at_ms,resolution_effective_at_ms,resolution_known_at_ms FROM history.asset_property_changes WHERE event_time_basis='device_recorded'").execute(&pool).await?;
    let mut cursor = None;
    let mut ids = Vec::new();
    loop {
        let page = view
            .list_property(&tenant, &query(0, 3000, 1, cursor))
            .await?;
        ids.extend(page.changes().iter().map(|c| c.id().clone()));
        cursor = page.next_after().cloned();
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(ids.len(), 3);
    assert_ne!(ids[0], ids[1]);
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn correction_does_not_retarget_old_history_and_late_data_requires_restart(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool).await?;
    let tenant = TenantId::new("tenant-a")?;
    let view = PostgresAssetPropertyChangeView::new(pool.clone());
    let page = view
        .list_property(&tenant, &query(0, 3000, 1, None))
        .await?;
    sqlx::raw_sql("UPDATE metadata.signal_bindings SET latest_revision=2 WHERE binding_id='binding-1'; INSERT INTO metadata.signal_binding_revisions (tenant_id,binding_id,revision,recorded_at_ms,source_device_asset_id,source_endpoint_id,source_signal_id,target_asset_id,target_property_id,target_property_version,effective_start_ms,effective_end_ms) SELECT tenant_id,binding_id,2,200,source_device_asset_id,source_endpoint_id,source_signal_id,'asset-b',target_property_id,target_property_version,effective_start_ms,effective_end_ms FROM metadata.signal_binding_revisions WHERE binding_id='binding-1'").execute(&pool).await?;
    sqlx::query("INSERT INTO history.asset_property_changes SELECT tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,2,event_time_basis,event_at_ms,resolution_effective_at_ms,250 FROM history.asset_property_changes WHERE binding_id='binding-1' AND event_time_basis='device_recorded'").execute(&pool).await?;
    assert_eq!(
        view.list_property(&tenant, &query(0, 3000, 100, None))
            .await?
            .changes()
            .len(),
        2
    );
    sqlx::query("INSERT INTO history.asset_property_changes SELECT tenant_id,message_id,protocol_package_id,protocol_package_version,1,binding_id,binding_revision,event_time_basis,500,500,150 FROM history.asset_property_changes WHERE binding_revision=1 AND event_time_basis='device_recorded'").execute(&pool).await?;
    let continued = view
        .list_property(&tenant, &query(0, 3000, 100, page.next_after().cloned()))
        .await?;
    assert_eq!(continued.changes().len(), 1);
    assert_eq!(continued.changes()[0].event_at().get(), 2000);
    let restarted = view
        .list_property(&tenant, &query(0, 3000, 100, None))
        .await?;
    assert_eq!(restarted.changes()[0].event_at().get(), 500);
    assert_eq!(restarted.changes().len(), 3);
    assert_eq!(restarted.changes()[1].id().binding_revision().get(), 1);
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn exact_property_version_does_not_merge_and_large_page_is_bounded(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool).await?;
    let tenant = TenantId::new("tenant-a")?;
    let view = PostgresAssetPropertyChangeView::new(pool.clone());
    sqlx::query("INSERT INTO history.asset_property_changes SELECT 'tenant-a','message-1','vendor.protocol','1',n,'binding-1',1,'device_recorded',1000,1000,150 FROM generate_series(2,106) AS n").execute(&pool).await?;
    let page = view
        .list_property(&tenant, &query(0, 3000, 100, None))
        .await?;
    assert_eq!(page.changes().len(), 100);
    assert!(page.next_after().is_some());
    let target = SignalTarget::new(
        AssetId::new("asset-a")?,
        PropertyDefinitionRef::new(
            PropertyDefinitionId::new("electrical.voltage")?,
            DefinitionVersion::new(2)?,
        ),
    );
    let q = PropertyHistoryQuery::new(
        target,
        UnixTimeMillis::new(0),
        UnixTimeMillis::new(3000),
        100,
        None,
    )?;
    assert!(view.list_property(&tenant, &q).await?.changes().is_empty());
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn decimal_history_preserves_arbitrary_precision_without_float_conversion(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    setup(&pool).await?;
    sqlx::raw_sql("INSERT INTO ingestion.accepted_messages (tenant_id,message_id,device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload) VALUES ('tenant-a','decimal-1','gateway-1','vendor.protocol','1',2000,'private-wire'::bytea);
        INSERT INTO ingestion.decoded_message_results (tenant_id,message_id,protocol_package_id,protocol_package_version,observation_count) VALUES ('tenant-a','decimal-1','vendor.protocol','1',1);
        INSERT INTO ingestion.decoded_observations (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,endpoint_id,signal_id,device_recorded_at_ms,value_kind,value_decimal,quality) VALUES ('tenant-a','decimal-1','vendor.protocol','1',0,'can-primary','exact',1000,'decimal',12345678901234567890.12345678901234567890123456789,'uncertain');
        INSERT INTO history.asset_property_changes (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis,event_at_ms,resolution_effective_at_ms,resolution_known_at_ms) VALUES ('tenant-a','decimal-1','vendor.protocol','1',0,'binding-decimal',1,'device_recorded',1000,1000,150);").execute(&pool).await?;
    let target = SignalTarget::new(
        AssetId::new("asset-a")?,
        PropertyDefinitionRef::new(
            PropertyDefinitionId::new("exact.decimal")?,
            DefinitionVersion::new(1)?,
        ),
    );
    let q = PropertyHistoryQuery::new(
        target,
        UnixTimeMillis::new(0),
        UnixTimeMillis::new(3000),
        100,
        None,
    )?;
    let page = PostgresAssetPropertyChangeView::new(pool)
        .list_property(&TenantId::new("tenant-a")?, &q)
        .await?;
    let fleetiq_domain::PropertyValue::Decimal(value) = page.changes()[0].observation().value()
    else {
        panic!("exact decimal should retain its kind");
    };
    assert_eq!(
        value.to_string(),
        "12345678901234567890.12345678901234567890123456789"
    );
    assert_eq!(
        page.changes()[0].observation().quality(),
        fleetiq_telemetry::ObservationQuality::Uncertain
    );
    Ok(())
}
