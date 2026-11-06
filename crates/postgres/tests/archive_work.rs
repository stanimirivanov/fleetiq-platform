//! Durable archive publication, fencing and late-arrival discovery.
use fleetiq_domain::TenantId;
use fleetiq_history::archive_work::{ArchiveClaim, ArchiveObject, ArchiveWorkRepository};
use fleetiq_postgres::PostgresArchiveWork;
use sqlx::PgPool;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn publication_is_verified_fenced_and_retryable(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::raw_sql(include_str!("property_history/topology.sql"))
        .execute(&pool)
        .await?;
    sqlx::raw_sql("INSERT INTO identity.actors VALUES ('archive-operator'); INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'archive-operator',7,30,1,2097152)").execute(&pool).await?;
    let tenant = TenantId::new("tenant-a")?;
    let repo = PostgresArchiveWork::new(pool.clone());
    let ArchiveClaim::Claimed(first) = repo.claim(&tenant).await? else {
        panic!("claim expected")
    };
    assert_eq!(first.changes.len(), 2);
    assert!(
        sqlx::query("UPDATE history.archive_jobs SET object_sha256=decode(repeat('00',32),'hex')")
            .execute(&pool)
            .await
            .is_err(),
        "partial object evidence must fail closed"
    );
    assert!(repo.manifest(&tenant, first.job_id).await?.is_none());
    assert!(matches!(repo.claim(&tenant).await?, ArchiveClaim::Denied));
    // A completed upload with no manifest commit leaves the frozen selection retryable.
    let object = ArchiveObject::new([7; 32], 1024)?;
    sqlx::query(
        "UPDATE history.archive_jobs SET lease_until=clock_timestamp()-interval '1 second'",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "UPDATE history.work_reservations SET expires_at=clock_timestamp()-interval '1 second'",
    )
    .execute(&pool)
    .await?;
    let ArchiveClaim::Claimed(next) = repo.claim(&tenant).await? else {
        panic!("retry expected")
    };
    assert_eq!(first.job_id, next.job_id);
    assert!(next.generation > first.generation);
    assert_eq!(first.changes, next.changes);
    assert!(!repo.complete(&first, object).await?);
    assert!(repo.complete(&next, object).await?);
    assert!(repo.complete(&next, object).await?);
    assert!(
        !repo
            .complete(&next, ArchiveObject::new([8; 32], 1024)?)
            .await?
    );
    let manifest = repo.manifest(&tenant, next.job_id).await?.unwrap();
    assert_eq!(manifest.object, object);
    assert_eq!(manifest.members, 2);
    assert!(
        repo.manifest(&TenantId::new("tenant-b")?, next.job_id)
            .await?
            .is_none()
    );
    assert!(matches!(repo.claim(&tenant).await?, ArchiveClaim::Idle));
    for sql in [
        "UPDATE history.archive_jobs SET object_bytes=1",
        "DELETE FROM history.archive_members",
        "DELETE FROM history.archive_jobs",
    ] {
        assert!(sqlx::query(sql).execute(&pool).await.is_err());
    }
    // A new identity with earlier event time must still be discovered.
    sqlx::raw_sql("INSERT INTO history.asset_property_changes SELECT tenant_id,message_id,protocol_package_id,protocol_package_version,1,binding_id,binding_revision,'device_recorded',500,resolution_effective_at_ms,resolution_known_at_ms FROM history.asset_property_changes ORDER BY event_time_basis LIMIT 1").execute(&pool).await?;
    let ArchiveClaim::Claimed(late) = repo.claim(&tenant).await? else {
        panic!("late identity expected")
    };
    assert_eq!(late.changes.len(), 1);
    assert_ne!(late.job_id, next.job_id);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn bounded_concurrent_selections_preserve_tenant_and_policy_fences(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::raw_sql(include_str!("property_history/topology.sql"))
        .execute(&pool)
        .await?;
    sqlx::raw_sql("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,2,4194304); INSERT INTO history.asset_property_changes SELECT tenant_id,message_id,protocol_package_id,protocol_package_version,n,'binding-1',1,'device_recorded',CASE WHEN n=1 THEN 500 ELSE 1000 END,1000,150 FROM ingestion.accepted_messages CROSS JOIN generate_series(1,106) n").execute(&pool).await?;
    let a = TenantId::new("tenant-a")?;
    let b = TenantId::new("tenant-b")?;
    let repo = PostgresArchiveWork::new(pool.clone());
    assert!(matches!(repo.claim(&b).await?, ArchiveClaim::Disabled));
    let (one, two) = tokio::join!(repo.claim(&a), repo.claim(&a));
    let (ArchiveClaim::Claimed(one), ArchiveClaim::Claimed(two)) = (one?, two?) else {
        panic!("two budgeted selections expected")
    };
    assert_ne!(one.job_id, two.job_id);
    assert_eq!(one.changes.len(), 16);
    assert_eq!(two.changes.len(), 16);
    assert!(
        one.changes
            .iter()
            .all(|c| two.changes.iter().all(|other| c.id() != other.id()))
    );
    let mut foreign = one.clone();
    foreign.tenant = b;
    assert!(
        !repo
            .complete(&foreign, ArchiveObject::new([1; 32], 1024)?)
            .await?
    );
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await?;
    assert!(
        !repo
            .complete(&one, ArchiveObject::new([1; 32], 1024)?)
            .await?
    );
    sqlx::query("UPDATE metadata.tenants SET is_active=true WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id) VALUES ('tenant-a',2,'operator-a')").execute(&pool).await?;
    assert!(
        !repo
            .complete(&one, ArchiveObject::new([1; 32], 1024)?)
            .await?
    );
    assert!(repo.manifest(&a, one.job_id).await?.is_none());
    Ok(())
}

struct UnavailableStore;
impl fleetiq_history::archive_work::ArchiveSegmentStore for UnavailableStore {
    fn store(
        &self,
        _: &TenantId,
        _: &[fleetiq_history::StoredPropertyChange],
    ) -> impl std::future::Future<
        Output = Result<ArchiveObject, fleetiq_history::archive_work::ArchiveFailure>,
    > + Send {
        std::future::ready(Err(fleetiq_history::archive_work::ArchiveFailure))
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn failed_verification_never_publishes_and_releases_budget(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::raw_sql(include_str!("property_history/topology.sql"))
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,1,2097152)").execute(&pool).await?;
    let tenant = TenantId::new("tenant-a")?;
    let repo = PostgresArchiveWork::new(pool.clone());
    assert!(
        fleetiq_history::archive_work::archive_once(&repo, &UnavailableStore, &tenant)
            .await
            .is_err()
    );
    let (published,reserved):(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM history.archive_jobs WHERE object_sha256 IS NOT NULL),(SELECT count(*) FROM history.work_reservations)").fetch_one(&pool).await?;
    assert_eq!((published, reserved), (0, 0));
    let ArchiveClaim::Claimed(retry) = repo.claim(&tenant).await? else {
        panic!("immediate exact retry expected")
    };
    assert_eq!(retry.changes.len(), 2);
    assert_eq!(retry.generation, 2);
    repo.abandon(&retry).await?;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn oversized_fields_stay_hot_without_freezing_unprocessable_jobs(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = include_str!("property_history/topology.sql")
        .replace("'integer'", "'text'")
        .replace("value_integer", "value_text")
        .replace("9223372036854775807", "repeat('x',9000)")
        .replace("'volt'", "NULL");
    // Only checked-in fixture text and fixed literal replacements; no external input.
    sqlx::raw_sql(sqlx::AssertSqlSafe(fixture))
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,1,2097152)").execute(&pool).await?;
    let repo = PostgresArchiveWork::new(pool.clone());
    assert!(matches!(
        repo.claim(&TenantId::new("tenant-a")?).await?,
        ArchiveClaim::Idle
    ));
    let (hot,members):(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM history.asset_property_changes),(SELECT count(*) FROM history.archive_members)").fetch_one(&pool).await?;
    assert_eq!((hot, members), (2, 0));
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn populated_upgrade_preserves_hot_evidence_and_starts_no_jobs(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    for migration in fleetiq_postgres::MIGRATOR.iter().filter(|m| m.version < 54) {
        sqlx::raw_sql(migration.sql.clone()).execute(&pool).await?;
    }
    sqlx::raw_sql(include_str!("property_history/topology.sql"))
        .execute(&pool)
        .await?;
    sqlx::raw_sql(include_str!(
        "../../../migrations/0054_archive_manifests.sql"
    ))
    .execute(&pool)
    .await?;
    let (hot,jobs,members):(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM history.asset_property_changes),(SELECT count(*) FROM history.archive_jobs),(SELECT count(*) FROM history.archive_members)").fetch_one(&pool).await?;
    assert_eq!((hot, jobs, members), (2, 0, 0));
    Ok(())
}
