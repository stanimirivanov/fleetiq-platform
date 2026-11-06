//! Real PostgreSQL lifecycle guards; no trigger is disabled by the executor.
use fleetiq_domain::TenantId;
use fleetiq_history::retention_work::{RetentionRepository, RetentionTarget, RetentionTick};
use fleetiq_postgres::PostgresHotRetention;
use sqlx::PgPool;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn reclamation_is_disabled_and_ordinary_mutations_remain_forbidden(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::raw_sql(include_str!("property_history/topology.sql"))
        .execute(&pool)
        .await?;
    let tenant = TenantId::new("tenant-a")?;
    let repo = PostgresHotRetention::new(pool.clone());
    let target = RetentionTarget::new(tenant, 1, 1)?;
    assert!(matches!(
        repo.inspect(&target).await?,
        fleetiq_history::retention_work::RetentionCheck::Finished(RetentionTick::Disabled)
    ));
    assert!(sqlx::query("UPDATE history.asset_property_changes SET event_at_ms=NULL,resolution_effective_at_ms=NULL,resolution_known_at_ms=NULL").execute(&pool).await.is_err());
    assert!(
        sqlx::query("DELETE FROM history.asset_property_changes")
            .execute(&pool)
            .await
            .is_err()
    );
    Ok(())
}
