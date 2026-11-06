//! Tenant isolation, revision fencing and immutable policy evidence.
use fleetiq_domain::{ActorId, TenantId};
use fleetiq_history::lifecycle::{LifecyclePolicy, LifecycleSettings};
use fleetiq_postgres::PostgresLifecyclePolicyRepository;
use sqlx::PgPool;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn policies_are_scoped_fenced_and_immutable(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::raw_sql("INSERT INTO metadata.tenants VALUES ('tenant-a'),('tenant-b'); INSERT INTO identity.actors VALUES ('operator-a');").execute(&pool).await?;
    let repo = PostgresLifecyclePolicyRepository::new(pool.clone());
    let a = TenantId::new("tenant-a")?;
    let b = TenantId::new("tenant-b")?;
    let actor = ActorId::new("operator-a")?;
    assert_eq!(repo.latest(&a).await?, (0, LifecycleSettings::Disabled));
    let enabled = LifecycleSettings::Configured(LifecyclePolicy::new(7, 30, 2, 1024)?);
    let (left, right) = tokio::join!(
        repo.append(&a, &actor, 0, enabled),
        repo.append(&a, &actor, 0, enabled)
    );
    assert_ne!(left.is_ok(), right.is_ok());
    assert_eq!(repo.latest(&a).await?, (1, enabled));
    assert_eq!(repo.latest(&b).await?, (0, LifecycleSettings::Disabled));
    assert!(
        repo.append(&a, &ActorId::new("missing")?, 1, enabled)
            .await
            .is_err()
    );
    repo.append(&a, &actor, 1, LifecycleSettings::Disabled)
        .await?;
    assert_eq!(repo.latest(&a).await?, (2, LifecycleSettings::Disabled));
    let attribution: String = sqlx::query_scalar(
        "SELECT actor_id FROM history.lifecycle_policies WHERE tenant_id='tenant-a' AND revision=1",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(attribution, "operator-a");
    for sql in [
        "UPDATE history.lifecycle_policies SET max_jobs=1",
        "DELETE FROM history.lifecycle_policies",
        "INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-b',1,'operator-a',20,10,1,1)",
    ] {
        assert!(sqlx::query(sql).execute(&pool).await.is_err());
    }
    Ok(())
}
