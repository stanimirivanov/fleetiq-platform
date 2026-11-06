//! Real concurrent admission and tenant isolation.
use fleetiq_domain::{ActorId, TenantId};
use fleetiq_history::lifecycle::{BudgetAdmission, LifecyclePolicy, LifecycleSettings};
use fleetiq_postgres::{PostgresHistoryBudgets, PostgresLifecyclePolicyRepository};
use sqlx::PgPool;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_reservations_cannot_oversubscribe_and_release_is_scoped(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::raw_sql("INSERT INTO metadata.tenants VALUES ('tenant-a'),('tenant-b'); INSERT INTO identity.actors VALUES ('operator-a');").execute(&pool).await?;
    let tenant = TenantId::new("tenant-a")?;
    let other = TenantId::new("tenant-b")?;
    let budgets = PostgresHistoryBudgets::new(pool.clone());
    assert_eq!(
        budgets.reserve(&tenant, 10).await?,
        BudgetAdmission::Disabled
    );
    PostgresLifecyclePolicyRepository::new(pool.clone())
        .append(
            &tenant,
            &ActorId::new("operator-a")?,
            0,
            LifecycleSettings::Configured(LifecyclePolicy::new(7, 30, 1, 10)?),
        )
        .await?;
    assert_eq!(budgets.reserve(&tenant, 11).await?, BudgetAdmission::Denied);
    assert!(budgets.reserve(&tenant, 0).await.is_err());
    let (a, b) = tokio::join!(budgets.reserve(&tenant, 10), budgets.reserve(&tenant, 10));
    let (a, b) = (a?, b?);
    let ((BudgetAdmission::Reserved(id), BudgetAdmission::Denied)
    | (BudgetAdmission::Denied, BudgetAdmission::Reserved(id))) = (a, b)
    else {
        panic!("exactly one reservation must win")
    };
    budgets.release(&other, id).await?;
    assert_eq!(budgets.reserve(&tenant, 1).await?, BudgetAdmission::Denied);
    budgets.release(&tenant, id).await?;
    let BudgetAdmission::Reserved(next) = budgets.reserve(&tenant, 10).await? else {
        panic!("released slot must be reusable")
    };
    assert_ne!(id, next);
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn byte_totals_policy_changes_and_expiry_are_reconciled(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::raw_sql("INSERT INTO metadata.tenants VALUES ('tenant-a'); INSERT INTO identity.actors VALUES ('operator-a');").execute(&pool).await?;
    let tenant = TenantId::new("tenant-a")?;
    let actor = ActorId::new("operator-a")?;
    let policies = PostgresLifecyclePolicyRepository::new(pool.clone());
    policies
        .append(
            &tenant,
            &actor,
            0,
            LifecycleSettings::Configured(LifecyclePolicy::new(7, 30, 3, 10)?),
        )
        .await?;
    let budgets = PostgresHistoryBudgets::new(pool.clone());
    let (a, b) = tokio::join!(budgets.reserve(&tenant, 6), budgets.reserve(&tenant, 6));
    assert!(matches!(
        (a?, b?),
        (BudgetAdmission::Reserved(_), BudgetAdmission::Denied)
            | (BudgetAdmission::Denied, BudgetAdmission::Reserved(_))
    ));
    policies
        .append(
            &tenant,
            &actor,
            1,
            LifecycleSettings::Configured(LifecyclePolicy::new(7, 30, 3, 5)?),
        )
        .await?;
    assert_eq!(budgets.reserve(&tenant, 1).await?, BudgetAdmission::Denied);
    sqlx::query("UPDATE history.work_reservations SET expires_at=clock_timestamp() WHERE tenant_id='tenant-a'").execute(&pool).await?;
    assert!(matches!(
        budgets.reserve(&tenant, 5).await?,
        BudgetAdmission::Reserved(_)
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.work_reservations")
            .fetch_one(&pool)
            .await?,
        1
    );
    Ok(())
}
