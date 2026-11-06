//! Trusted producer remains opt-in and publishes exact bounded derived evidence.
use fleetiq_domain::{
    AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, SignalTarget, TenantId,
};
use fleetiq_history::downsampling::{NumericRequest, NumericSpec, SummaryFailure};
use sqlx::PgPool;
fn request(
    property: &str,
    start: i64,
    end: i64,
    width: i64,
    restore: Option<fleetiq_history::restore::RestoreTicket>,
) -> NumericRequest {
    NumericRequest::new(
        TenantId::new("tenant-a").unwrap(),
        SignalTarget::new(
            AssetId::new("asset-a").unwrap(),
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new(property).unwrap(),
                DefinitionVersion::new(1).unwrap(),
            ),
        ),
        NumericSpec::new(start, end, width).unwrap(),
        restore,
    )
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn producer_is_disabled_without_policy_and_concurrent_retries_share_a_revision(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::raw_sql(include_str!(
        "../../postgres/tests/property_history/topology.sql"
    ))
    .execute(&pool)
    .await?;
    let req = request("electrical.voltage", 0, 3000, 1000, None);
    assert_eq!(
        fleetiq_server::produce_numeric_summary_once(pool.clone(), &req).await,
        Err(SummaryFailure::Disabled)
    );
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,3,8388608)").execute(&pool).await?;
    let (a, b) = tokio::join!(
        fleetiq_server::produce_numeric_summary_once(pool.clone(), &req),
        fleetiq_server::produce_numeric_summary_once(pool.clone(), &req)
    );
    let a = a?;
    assert_eq!(a, b?);
    let body: serde_json::Value = sqlx::query_scalar(
        "SELECT body FROM history.numeric_summaries WHERE tenant_id='tenant-a' AND series_id=$1",
    )
    .bind(a.series)
    .fetch_one(&pool)
    .await?;
    assert_eq!(body["buckets"][1]["sum"], "9223372036854775807");
    assert_eq!(body["buckets"][1]["count"], 1);
    assert_eq!(body["coverage"]["scope"], "hot_snapshot");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.work_reservations")
            .fetch_one(&pool)
            .await?,
        0
    );
    assert_eq!(
        fleetiq_server::produce_numeric_summary_once(
            pool.clone(),
            &request("switch.state", 0, 3000, 1000, None)
        )
        .await,
        Err(SummaryFailure::InvalidEvidence)
    );
    sqlx::query("INSERT INTO history.asset_property_changes SELECT tenant_id,message_id,protocol_package_id,protocol_package_version,n,'binding-1',1,'device_recorded',CASE WHEN n=1 THEN 500 ELSE 1000 END,1000,150 FROM ingestion.accepted_messages CROSS JOIN generate_series(1,100) n").execute(&pool).await?;
    assert_eq!(
        fleetiq_server::produce_numeric_summary_once(pool.clone(), &req).await,
        Err(SummaryFailure::InputLimit)
    );
    Ok(())
}

#[path = "hot_retention/support.rs"]
mod retained;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn reclaimed_history_requires_ready_selected_restore_and_deduplicates_originals(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    use fleetiq_history::{
        restore::{RestoreClaim, RestoreRepository, RestoredSegment},
        retention_work::RetentionRepository,
    };
    let (target, original, bytes) = retained::selection(&pool).await?;
    retained::quiesce(&pool).await?;
    let retention = fleetiq_postgres::PostgresHotRetention::new(pool.clone());
    retention.approve(target.tenant(), 1, "operator-a").await?;
    let segment = RestoredSegment::new(bytes, original)?;
    retention
        .reclaim(&target, &retained::Source(&segment))
        .await?;
    let hot = request("electrical.voltage", 1500, 2500, 500, None);
    assert_eq!(
        fleetiq_server::produce_numeric_summary_once(pool.clone(), &hot).await,
        Err(SummaryFailure::CoverageUnavailable)
    );
    let restores = fleetiq_postgres::PostgresRestore::new(pool.clone());
    let ticket = restores
        .request(target.tenant(), target.manifest(), 0)
        .await?;
    let selected = request("electrical.voltage", 1500, 2500, 500, Some(ticket));
    assert_eq!(
        fleetiq_server::produce_numeric_summary_once(pool.clone(), &selected).await,
        Err(SummaryFailure::CoverageUnavailable)
    );
    let RestoreClaim::Claimed(lease) = restores.claim(target.tenant(), ticket).await? else {
        panic!("restore");
    };
    assert!(restores.complete(&lease, &segment).await?);
    let ready = fleetiq_server::produce_numeric_summary_once(pool.clone(), &selected).await?;
    let body: serde_json::Value =
        sqlx::query_scalar("SELECT body FROM history.numeric_summaries WHERE series_id=$1")
            .bind(ready.series)
            .fetch_one(&pool)
            .await?;
    assert_eq!(body["buckets"][1]["count"], 2);
    assert_eq!(body["buckets"][1]["sum"], "18446744073709551614");
    assert_eq!(
        body["coverage"]["scope"],
        "hot_and_selected_manifest_snapshot"
    );
    restores.cancel(target.tenant(), &ticket).await?;
    assert_eq!(
        fleetiq_server::produce_numeric_summary_once(pool.clone(), &selected).await,
        Err(SummaryFailure::CoverageUnavailable)
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.numeric_summaries")
            .fetch_one(&pool)
            .await?,
        1
    );
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn decimal_production_preserves_precision_and_quality_counts(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::raw_sql(include_str!(
        "../../postgres/tests/property_history/topology.sql"
    ))
    .execute(&pool)
    .await?;
    sqlx::raw_sql("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,3,8388608);
 INSERT INTO ingestion.accepted_messages(tenant_id,message_id,device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload) SELECT tenant_id,'decimal-message',device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload FROM ingestion.accepted_messages;
 INSERT INTO ingestion.decoded_message_results VALUES ('tenant-a','decimal-message','vendor.protocol','1',2);
 INSERT INTO ingestion.decoded_observations(tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,endpoint_id,signal_id,device_recorded_at_ms,value_kind,value_decimal,quality)
 VALUES ('tenant-a','decimal-message','vendor.protocol','1',0,'can-primary','exact',1000,'decimal',9007199254740993.00000000000000000001,'good'),('tenant-a','decimal-message','vendor.protocol','1',1,'can-primary','exact',2000,'decimal',0.00000000000000000002,'uncertain');
 INSERT INTO history.asset_property_changes VALUES ('tenant-a','decimal-message','vendor.protocol','1',0,'binding-decimal',1,'device_recorded',1000,1000,150),('tenant-a','decimal-message','vendor.protocol','1',1,'binding-decimal',1,'device_recorded',2000,1000,150)").execute(&pool).await?;
    let receipt = fleetiq_server::produce_numeric_summary_once(
        pool.clone(),
        &request("exact.decimal", 0, 3000, 1000, None),
    )
    .await?;
    let body: serde_json::Value =
        sqlx::query_scalar("SELECT body FROM history.numeric_summaries WHERE series_id=$1")
            .bind(receipt.series)
            .fetch_one(&pool)
            .await?;
    assert_eq!(
        body["buckets"][1]["sum"],
        "9007199254740993.00000000000000000001"
    );
    assert_eq!(body["buckets"][2]["excluded"], 1);
    assert_eq!(body["buckets"][2]["count"], 0);
    assert!(body["buckets"][2]["min"].is_null());
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn uninstalled_property_version_is_not_a_valid_empty_scope(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::raw_sql(include_str!(
        "../../postgres/tests/property_history/topology.sql"
    ))
    .execute(&pool)
    .await?;
    sqlx::query("INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,2,4194304)").execute(&pool).await?;
    let req = NumericRequest::new(
        TenantId::new("tenant-a")?,
        SignalTarget::new(
            AssetId::new("asset-a")?,
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new("electrical.voltage")?,
                DefinitionVersion::new(2)?,
            ),
        ),
        NumericSpec::new(0, 3000, 1000)?,
        None,
    );
    assert_eq!(req.target().property().version().get(), 2);
    let installed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM metadata.assets a JOIN metadata.asset_type_properties p ON p.tenant_id=a.tenant_id AND p.type_id=a.type_id AND p.type_version=a.type_version WHERE a.tenant_id='tenant-a' AND a.asset_id='asset-a' AND p.property_id='electrical.voltage' AND p.property_version=2)").fetch_one(&pool).await?;
    assert!(!installed, "negative fixture must be uninstalled");
    assert_eq!(
        fleetiq_server::produce_numeric_summary_once(pool.clone(), &req).await,
        Err(SummaryFailure::InvalidEvidence)
    );
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn inactive_tenant_is_reported_as_disabled(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::raw_sql(include_str!(
        "../../postgres/tests/property_history/topology.sql"
    ))
    .execute(&pool)
    .await?;
    sqlx::query("INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,2,1)").execute(&pool).await?;
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await?;
    assert_eq!(
        fleetiq_server::produce_numeric_summary_once(
            pool.clone(),
            &request("electrical.voltage", 0, 3000, 1000, None)
        )
        .await,
        Err(SummaryFailure::Disabled)
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn hot_and_ready_overlap_counts_each_interpretation_once(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    use fleetiq_history::{
        archive_work::{ArchiveClaim, ArchiveObject, ArchiveWorkRepository},
        restore::{RestoreClaim, RestoreRepository, RestoredSegment},
    };
    use sha2::{Digest, Sha256};
    sqlx::raw_sql(include_str!(
        "../../postgres/tests/property_history/topology.sql"
    ))
    .execute(&pool)
    .await?;
    sqlx::query("INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,3,8388608)").execute(&pool).await?;
    let tenant = TenantId::new("tenant-a")?;
    let archive = fleetiq_postgres::PostgresArchiveWork::new(pool.clone());
    let ArchiveClaim::Claimed(lease) = archive.claim(&tenant).await? else {
        panic!("archive");
    };
    let bytes = fleetiq_archive::encode(&tenant, &lease.changes)?;
    let object = ArchiveObject::new(Sha256::digest(&bytes).into(), i64::try_from(bytes.len())?)?;
    archive.complete(&lease, object).await?;
    let restores = fleetiq_postgres::PostgresRestore::new(pool.clone());
    let ticket = restores.request(&tenant, lease.job_id, 0).await?;
    let RestoreClaim::Claimed(restoring) = restores.claim(&tenant, ticket).await? else {
        panic!("restore");
    };
    restores
        .complete(&restoring, &RestoredSegment::new(bytes, lease.changes)?)
        .await?;
    let receipt = fleetiq_server::produce_numeric_summary_once(
        pool.clone(),
        &request("electrical.voltage", 0, 3000, 1000, Some(ticket)),
    )
    .await?;
    let body: serde_json::Value =
        sqlx::query_scalar("SELECT body FROM history.numeric_summaries WHERE series_id=$1")
            .bind(receipt.series)
            .fetch_one(&pool)
            .await?;
    assert_eq!(
        body["inputs"].as_array().unwrap().len(),
        2,
        "overlap must never double count"
    );
    assert_eq!(body["buckets"][1]["count"], 1);
    assert_eq!(body["buckets"][2]["count"], 1);
    Ok(())
}
