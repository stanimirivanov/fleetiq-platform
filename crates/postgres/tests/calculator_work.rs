//! Real-database scheduling, fencing, retries and atomic completion.
use fleetiq_analytics::*;
use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use fleetiq_postgres::{PostgresCalculatorPreparation, PostgresCalculatorWork};
use sqlx::PgPool;
use std::error::Error;
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
fn schedule() -> CalculatorSchedule {
    CalculatorSchedule {
        tenant: TenantId::new("tenant-a").unwrap(),
        id: CalculatorConfigurationId::new("schedule").unwrap(),
        calculator: CalculatorRef::new(
            CalculatorId::new("mean").unwrap(),
            CalculatorVersion::new("1").unwrap(),
        ),
        configuration: CalculatorConfigurationId::new("scheduled-config").unwrap(),
        scope: AssetId::new("asset-a").unwrap(),
        target: AssetId::new("result").unwrap(),
        first_start: UnixTimeMillis::new(0),
        window_ms: 3000,
        lateness_ms: 0,
        minimum_samples: 1,
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn registration_and_concurrent_catchup_are_bounded_and_immutable(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let work = PostgresCalculatorWork::new(pool.clone());
    let original = schedule();
    work.register(&original).await?;
    work.register(&original).await?;
    let mut changed = original.clone();
    changed.minimum_samples = 2;
    assert!(work.register(&changed).await.is_err());
    changed = original.clone();
    changed.tenant = TenantId::new("tenant-b")?;
    assert!(work.register(&changed).await.is_err());
    sqlx::raw_sql(include_str!("calculator_configuration/activate.sql"))
        .execute(&pool)
        .await?;
    let (a, b) = tokio::join!(work.seed(), work.seed());
    let count = a? + b?;
    assert!(count == 32 || count == 64);
    let (rows, cursor): (i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM analytics.calculator_work),next_start_ms FROM analytics.calculator_schedules").fetch_one(&pool).await?;
    assert_eq!(rows, count);
    assert_eq!(cursor, count * 3000);
    assert!(
        sqlx::query("UPDATE analytics.calculator_schedules SET minimum_samples=2")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE analytics.calculator_work SET request='{}'")
            .execute(&pool)
            .await
            .is_err()
    );
    let (a, b) = tokio::join!(work.claim(), work.claim());
    assert_ne!(
        a?.unwrap().request.evaluation.id(),
        b?.unwrap().request.evaluation.id()
    );
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn stale_and_expired_claims_cannot_publish_and_retries_reuse_preparation(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let work = PostgresCalculatorWork::new(pool.clone());
    work.register(&schedule()).await?;
    assert_eq!(fixture_seed(&pool, &work).await?, 32);
    let first = work.claim().await?.unwrap();
    let preparation = PostgresCalculatorPreparation::new(pool.clone());
    let prepared = preparation.prepare(&first.request).await?;
    assert_eq!(prepared.samples.len(), 2);
    let record = calculate_mean(prepared.clone())?;
    sqlx::query("UPDATE analytics.calculator_work SET lease_until_ms=0 WHERE evaluation_id=$1")
        .bind(first.request.evaluation.id().as_str())
        .execute(&pool)
        .await?;
    assert!(work.complete(&first, &record).await.is_err());
    let second = work.claim().await?.unwrap();
    assert_eq!(first.request, second.request);
    assert_eq!(second.generation, 2);
    assert_eq!(
        preparation.prepare(&second.request).await?.manifest,
        prepared.manifest
    );
    assert!(work.complete(&first, &record).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.evaluations")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 0);
    work.complete(&second, &record).await?;
    let status: String =
        sqlx::query_scalar("SELECT status FROM analytics.calculator_work WHERE evaluation_id=$1")
            .bind(second.request.evaluation.id().as_str())
            .fetch_one(&pool)
            .await?;
    assert_eq!(status, "done");
    assert!(work.complete(&second, &record).await.is_err());
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn failed_checkpoint_rolls_back_terminal_output(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let work = PostgresCalculatorWork::new(pool.clone());
    work.register(&schedule()).await?;
    fixture_seed(&pool, &work).await?;
    let lease = work.claim().await?.unwrap();
    let record = calculate_mean(
        PostgresCalculatorPreparation::new(pool.clone())
            .prepare(&lease.request)
            .await?,
    )?;
    sqlx::raw_sql("CREATE FUNCTION analytics.reject_test_completion() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.status='done' THEN RAISE EXCEPTION 'test checkpoint failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_test_completion BEFORE UPDATE ON analytics.calculator_work FOR EACH ROW EXECUTE FUNCTION analytics.reject_test_completion();").execute(&pool).await?;
    assert!(work.complete(&lease, &record).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.evaluations")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 0);
    sqlx::query("DROP TRIGGER reject_test_completion ON analytics.calculator_work")
        .execute(&pool)
        .await?;
    work.complete(&lease, &record).await?;
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn retries_exhaust_after_three_generations_without_sleeping(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let work = PostgresCalculatorWork::new(pool.clone());
    work.register(&schedule()).await?;
    fixture_seed(&pool, &work).await?;
    for generation in 1..=3 {
        let lease = work.claim().await?.unwrap();
        assert_eq!(lease.generation, generation);
        assert_eq!(lease.request.window.start(), UnixTimeMillis::new(0));
        sqlx::query("UPDATE analytics.calculator_work SET lease_until_ms=0 WHERE evaluation_id=$1")
            .bind(lease.request.evaluation.id().as_str())
            .execute(&pool)
            .await?;
    }
    let next = work.claim().await?.unwrap();
    assert_eq!(next.generation, 1);
    let status: String =
        sqlx::query_scalar("SELECT status FROM analytics.calculator_work WHERE start_ms=0")
            .fetch_one(&pool)
            .await?;
    assert_eq!(status, "exhausted");
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn expiry_during_append_rolls_back_both_terminal_and_checkpoint(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let work = PostgresCalculatorWork::new(pool.clone());
    work.register(&schedule()).await?;
    fixture_seed(&pool, &work).await?;
    let lease = work.claim().await?.unwrap();
    let record = calculate_mean(
        PostgresCalculatorPreparation::new(pool.clone())
            .prepare(&lease.request)
            .await?,
    )?;
    sqlx::raw_sql("CREATE FUNCTION analytics.expire_test_claim() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN UPDATE analytics.calculator_work SET lease_until_ms=0 WHERE tenant_id=NEW.tenant_id AND evaluation_id=NEW.evaluation_id; RETURN NEW; END $$; CREATE TRIGGER expire_test_claim AFTER INSERT ON analytics.evaluations FOR EACH ROW EXECUTE FUNCTION analytics.expire_test_claim();").execute(&pool).await?;
    assert!(work.complete(&lease, &record).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.evaluations")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 0);
    let status: String =
        sqlx::query_scalar("SELECT status FROM analytics.calculator_work WHERE start_ms=0")
            .fetch_one(&pool)
            .await?;
    assert_eq!(status, "pending");
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn suspended_tenant_cannot_seed_claim_or_complete(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let work = PostgresCalculatorWork::new(pool.clone());
    work.register(&schedule()).await?;
    fixture_seed(&pool, &work).await?;
    let lease = work.claim().await?.unwrap();
    let record = calculate_mean(
        PostgresCalculatorPreparation::new(pool.clone())
            .prepare(&lease.request)
            .await?,
    )?;
    sqlx::query("UPDATE metadata.tenants SET is_active=FALSE")
        .execute(&pool)
        .await?;
    assert_eq!(work.seed().await?, 0);
    assert!(work.claim().await?.is_none());
    assert!(work.complete(&lease, &record).await.is_err());
    sqlx::query("UPDATE metadata.tenants SET is_active=TRUE")
        .execute(&pool)
        .await?;
    work.complete(&lease, &record).await?;
    Ok(())
}

async fn fixture_seed(
    pool: &PgPool,
    work: &fleetiq_postgres::PostgresCalculatorWork,
) -> Result<i64, Box<dyn Error>> {
    sqlx::raw_sql(include_str!("calculator_configuration/activate.sql"))
        .execute(pool)
        .await?;
    Ok(work.seed().await?)
}
