use super::*;
use fleetiq_analytics::AlertDeliveryFailure;
use fleetiq_postgres::PostgresAlertDelivery;
const DEST: [u8; 32] = [7; 32];
async fn fire(pool: &PgPool) -> Result<i64, Box<dyn Error>> {
    let base = setup(pool, 0, 0).await?;
    append(pool, "fire", base, Some("51")).await?;
    assert!(
        PostgresAlertLifecycle::new(pool.clone())
            .advance_one()
            .await?
    );
    Ok(base)
}
async fn due(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE analytics.alert_notification_delivery SET lease_until=clock_timestamp()-interval '1 second',next_attempt_at=clock_timestamp()-interval '1 second'").execute(pool).await?;
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn restart_refresh_and_resolution_preserve_incident_and_immutable_intents(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let base = fire(&pool).await?;
    let work = PostgresAlertDelivery::new(pool.clone());
    work.seed(&DEST).await?;
    let first = work.claim(&DEST).await?.ok_or("missing firing")?;
    assert_eq!(first.phase, AlertPhase::Firing);
    assert!(work.finish(&first, Ok(())).await?);
    assert!(work.claim(&DEST).await?.is_none());
    due(&pool).await?;
    let refresh = PostgresAlertDelivery::new(pool.clone())
        .claim(&DEST)
        .await?
        .ok_or("missing refresh")?;
    assert_eq!(refresh.firing_version, first.firing_version);
    assert_eq!(refresh.starts_at, first.starts_at);
    assert!(refresh.generation > first.generation);
    assert!(!work.finish(&first, Ok(())).await?);
    append(&pool, "resolve", base + 1000, Some("49")).await?;
    assert!(
        PostgresAlertLifecycle::new(pool.clone())
            .advance_one()
            .await?
    );
    due(&pool).await?;
    let resolved = work.claim(&DEST).await?.ok_or("missing resolution")?;
    assert_eq!(resolved.phase, AlertPhase::Resolved);
    assert!(resolved.version > first.version);
    assert_eq!(resolved.firing_version, first.firing_version);
    assert!(!work.finish(&refresh, Ok(())).await?);
    assert!(work.finish(&resolved, Ok(())).await?);
    due(&pool).await?;
    let healing = work.claim(&DEST).await?.ok_or("missing healing")?;
    assert_eq!(healing.version, resolved.version);
    assert!(work.finish(&healing, Ok(())).await?);
    sqlx::query("UPDATE analytics.alert_notification_delivery SET resolved_until=clock_timestamp()-interval '1 second'").execute(&pool).await?;
    due(&pool).await?;
    assert!(work.claim(&DEST).await?.is_none());
    let counts = counts(&pool).await;
    assert_eq!(counts, (2, 2, 2));
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn resolved_backlog_never_reopens_and_new_endpoint_has_independent_position(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let base = fire(&pool).await?;
    append(&pool, "resolve", base + 1000, Some("49")).await?;
    PostgresAlertLifecycle::new(pool.clone())
        .advance_one()
        .await?;
    let work = PostgresAlertDelivery::new(pool.clone());
    work.seed(&DEST).await?;
    let lease = work
        .claim(&DEST)
        .await?
        .ok_or("missing coalesced resolution")?;
    assert_eq!(lease.phase, AlertPhase::Resolved);
    assert_eq!(lease.starts_at.get(), base + 1000);
    assert!(work.finish(&lease, Ok(())).await?);
    let other = [8; 32];
    work.seed(&other).await?;
    assert!(work.claim(&other).await?.is_some());
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_claims_expiry_exhaustion_and_new_resolution_are_fenced(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let base = fire(&pool).await?;
    let a = PostgresAlertDelivery::new(pool.clone());
    let b = PostgresAlertDelivery::new(pool.clone());
    a.seed(&DEST).await?;
    let (x, y) = tokio::join!(a.claim(&DEST), b.claim(&DEST));
    let x = x?;
    let y = y?;
    assert_eq!(usize::from(x.is_some()) + usize::from(y.is_some()), 1);
    let first = x.or(y).ok_or("missing winner")?;
    due(&pool).await?;
    assert!(!a.finish(&first, Ok(())).await?);
    for _ in 0..4 {
        let lease = a.claim(&DEST).await?.ok_or("missing retry")?;
        a.finish(&lease, Err(AlertDeliveryFailure::Transport))
            .await?;
        due(&pool).await?;
    }
    assert!(a.claim(&DEST).await?.is_none());
    append(&pool, "resolve", base + 1000, Some("49")).await?;
    PostgresAlertLifecycle::new(pool.clone())
        .advance_one()
        .await?;
    let resolved = a
        .claim(&DEST)
        .await?
        .ok_or("resolution must rearm exhausted firing")?;
    assert_eq!(resolved.phase, AlertPhase::Resolved);
    assert!(a.finish(&resolved, Ok(())).await?);
    assert!(!a.finish(&first, Ok(())).await?);
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn inactive_tenant_cannot_claim_or_commit_and_incident_identity_cannot_regress(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    fire(&pool).await?;
    let work = PostgresAlertDelivery::new(pool.clone());
    work.seed(&DEST).await?;
    let lease = work.claim(&DEST).await?.ok_or("missing claim")?;
    let mut foreign = lease.clone();
    foreign.tenant = TenantId::new("tenant-b")?;
    assert!(!work.finish(&foreign, Ok(())).await?);
    assert!(sqlx::query("INSERT INTO analytics.alert_notification_delivery(destination,tenant_id,rule_id,revision,firing_version,target_version) SELECT destination,'tenant-b',rule_id,revision,firing_version,target_version FROM analytics.alert_notification_delivery").execute(&pool).await.is_err());
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await?;
    assert!(!work.finish(&lease, Ok(())).await?);
    due(&pool).await?;
    assert!(work.claim(&DEST).await?.is_none());
    assert!(
        sqlx::query("UPDATE analytics.alert_notification_delivery SET generation=0")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM analytics.alert_notification_delivery")
            .execute(&pool)
            .await
            .is_err()
    );
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn populated_48_to_49_upgrade_retains_intents_and_requires_explicit_seed(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    for migration in fleetiq_postgres::MIGRATOR.iter().filter(|m| m.version < 49) {
        sqlx::raw_sql(migration.sql.clone()).execute(&pool).await?;
    }
    let base = fire(&pool).await?;
    append(&pool, "resolve", base + 1000, Some("49")).await?;
    PostgresAlertLifecycle::new(pool.clone())
        .advance_one()
        .await?;
    let before = counts(&pool).await;
    sqlx::raw_sql(include_str!(
        "../../../../migrations/0049_alert_notification_delivery.sql"
    ))
    .execute(&pool)
    .await?;
    assert_eq!(before, counts(&pool).await);
    let queued: i64 =
        sqlx::query_scalar("SELECT count(*) FROM analytics.alert_notification_delivery")
            .fetch_one(&pool)
            .await?;
    assert_eq!(queued, 0);
    let work = PostgresAlertDelivery::new(pool);
    work.seed(&DEST).await?;
    let lease = work
        .claim(&DEST)
        .await?
        .ok_or("missing upgraded incident")?;
    assert_eq!(lease.phase, AlertPhase::Resolved);
    Ok(())
}
