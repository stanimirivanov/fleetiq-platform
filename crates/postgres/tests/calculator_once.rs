//! Preparation is durable across late arrivals and ambiguous terminal commits.
use fleetiq_analytics::*;
use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use fleetiq_postgres::{
    EvaluationStoreError, PostgresCalculatorEvaluationStore, PostgresCalculatorPreparation,
};
use sqlx::PgPool;
use std::{
    error::Error,
    sync::atomic::{AtomicBool, Ordering},
};
fn request(id: &str) -> CalculatorRunRequest {
    let time = UnixTimeMillis::new;
    CalculatorRunRequest {
        evaluation: CalculatorEvaluation::new(
            CalculatorEvaluationId::new(id).unwrap(),
            TenantId::new("tenant-a").unwrap(),
            AssetId::new("asset-a").unwrap(),
            CalculatorRef::new(
                CalculatorId::new("mean").unwrap(),
                CalculatorVersion::new("1").unwrap(),
            ),
            time(3000),
            time(3500),
        ),
        configuration: CalculatorConfigurationId::new(id).unwrap(),
        window: CalculatorWindow::new(time(0), time(3000), time(3000)).unwrap(),
        known_at: time(3000),
        minimum_samples: 1,
        target: AssetId::new("result").unwrap(),
    }
}
async fn seed(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(include_str!("property_history/topology.sql"))
        .execute(pool)
        .await?;
    sqlx::raw_sql("INSERT INTO metadata.property_definitions (tenant_id,property_id,version,display_name,value_kind,canonical_unit) VALUES ('tenant-a','mean.voltage',1,'Mean','decimal','volt');
      INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count) VALUES ('tenant-a','calculated',1,'Calculated',1);
      INSERT INTO metadata.asset_type_properties (tenant_id,type_id,type_version,property_id,property_version,property_ordinal) VALUES ('tenant-a','calculated',1,'mean.voltage',1,1);
      INSERT INTO metadata.assets (tenant_id,asset_id,type_id,type_version,display_name) VALUES ('tenant-a','result','calculated',1,'Result');
      INSERT INTO analytics.calculator_definitions VALUES ('tenant-a','mean','1','window-mean-v1','electrical.voltage',1,'mean.voltage',1);").execute(pool).await?;
    Ok(())
}
struct LostCommit {
    store: PostgresCalculatorEvaluationStore,
    lost: AtomicBool,
}
impl CalculatorEvaluationStore for LostCommit {
    type Error = EvaluationStoreError;
    async fn append(
        &self,
        record: &CalculatorEvaluationRecord,
    ) -> Result<EvaluationAppendOutcome, Self::Error> {
        let result = self.store.append(record).await?;
        if self.lost.swap(false, Ordering::SeqCst) {
            return Err(EvaluationStoreError::Database);
        }
        Ok(result)
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn retry_reuses_prepared_inputs_after_late_arrival_and_lost_commit(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let preparation = PostgresCalculatorPreparation::new(pool.clone());
    let store = LostCommit {
        store: PostgresCalculatorEvaluationStore::new(pool.clone()),
        lost: AtomicBool::new(true),
    };
    let original = request("original");
    let (first, second) = tokio::join!(
        preparation.prepare(&original),
        preparation.prepare(&original)
    );
    assert_eq!(first?.manifest, second?.manifest);
    assert_eq!(
        run_calculator_once(&preparation, &store, &original).await,
        Err(CalculatorRunError::Storage)
    );
    sqlx::raw_sql("INSERT INTO ingestion.accepted_messages (tenant_id,message_id,device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload) SELECT tenant_id,'late',device_asset_id,protocol_package_id,protocol_package_version,2500,payload FROM ingestion.accepted_messages WHERE message_id='message-1';
      INSERT INTO ingestion.decoded_message_results VALUES ('tenant-a','late','vendor.protocol','1',1);
      INSERT INTO ingestion.decoded_observations (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,endpoint_id,signal_id,device_recorded_at_ms,value_kind,value_integer,source_unit,quality) SELECT tenant_id,'late',protocol_package_id,protocol_package_version,0,endpoint_id,signal_id,1500,value_kind,0,source_unit,quality FROM ingestion.decoded_observations WHERE message_id='message-1' AND output_index=0;
      INSERT INTO history.asset_property_changes VALUES ('tenant-a','late','vendor.protocol','1',0,'binding-1',1,'device_recorded',1500,1500,150);").execute(&pool).await?;
    assert_eq!(
        run_calculator_once(&preparation, &store, &original).await?,
        EvaluationAppendOutcome::AlreadyPresent
    );
    assert_eq!(preparation.prepare(&original).await?.samples.len(), 2);
    let fresh = request("fresh");
    assert_eq!(preparation.prepare(&fresh).await?.samples.len(), 3);
    assert_eq!(
        run_calculator_once(&preparation, &store, &fresh).await?,
        EvaluationAppendOutcome::Appended
    );
    let mean: String = sqlx::query_scalar(
        "SELECT outputs->0->>'value' FROM analytics.evaluations WHERE evaluation_id='fresh'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(mean, "6148914691236517204.666666666666666666");
    let mut changed = original.clone();
    changed.minimum_samples = 2;
    assert!(preparation.prepare(&changed).await.is_err());
    let mut invalid = request("bad-target");
    invalid.target = AssetId::new("asset-a")?;
    assert!(preparation.prepare(&invalid).await.is_err());
    let mut empty = request("empty");
    empty.window = CalculatorWindow::new(
        UnixTimeMillis::new(2100),
        UnixTimeMillis::new(3000),
        UnixTimeMillis::new(3000),
    )?;
    assert_eq!(
        run_calculator_once(&preparation, &store, &empty).await?,
        EvaluationAppendOutcome::Appended
    );
    let outcome: String =
        sqlx::query_scalar("SELECT outcome FROM analytics.evaluations WHERE evaluation_id='empty'")
            .fetch_one(&pool)
            .await?;
    assert_eq!(outcome, "insufficient");
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn overflow_rejects_without_pinning_a_partial_selection(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    sqlx::raw_sql("INSERT INTO ingestion.accepted_messages (tenant_id,message_id,device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload) SELECT tenant_id,'many',device_asset_id,protocol_package_id,protocol_package_version,2500,payload FROM ingestion.accepted_messages WHERE message_id='message-1';
      INSERT INTO ingestion.decoded_message_results VALUES ('tenant-a','many','vendor.protocol','1',1025);
      INSERT INTO ingestion.decoded_observations (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,endpoint_id,signal_id,device_recorded_at_ms,value_kind,value_integer,source_unit,quality) SELECT o.tenant_id,'many',o.protocol_package_id,o.protocol_package_version,n,o.endpoint_id,o.signal_id,1500,o.value_kind,1,o.source_unit,o.quality FROM ingestion.decoded_observations o CROSS JOIN generate_series(0,1024) n WHERE message_id='message-1' AND output_index=0;
      INSERT INTO history.asset_property_changes SELECT 'tenant-a','many','vendor.protocol','1',n,'binding-1',1,'device_recorded',1500,1500,150 FROM generate_series(0,1024) n;").execute(&pool).await?;
    let preparation = PostgresCalculatorPreparation::new(pool.clone());
    assert!(preparation.prepare(&request("overflow")).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.prepared_evaluations")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 0);
    Ok(())
}
