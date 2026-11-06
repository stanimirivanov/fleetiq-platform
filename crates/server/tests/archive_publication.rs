//! Real PostgreSQL plus pinned S3: upload/commit uncertainty and verification failure.
#[path = "../../archive/tests/support/provider.rs"]
mod provider;
use fleetiq_archive::objects::ImmutableArchiveObjects;
use fleetiq_domain::TenantId;
use fleetiq_history::archive_work::{
    ArchiveClaim, ArchiveSegmentStore, ArchiveTick, ArchiveWorkRepository,
};
use fleetiq_postgres::PostgresArchiveWork;
use sqlx::PgPool;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn verified_object_publication_survives_upload_commit_gap(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::raw_sql(include_str!(
        "../../postgres/tests/property_history/topology.sql"
    ))
    .execute(&pool)
    .await?;
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,1,2097152)").execute(&pool).await?;
    let fixture = provider::Provider::start().await;
    let tenant = TenantId::new("tenant-a")?;
    let repo = PostgresArchiveWork::new(pool.clone());
    let objects = ImmutableArchiveObjects::connect(fixture.config())?;
    let ArchiveClaim::Claimed(lease) = repo.claim(&tenant).await? else {
        panic!("selection expected")
    };
    let stored = objects.store(&tenant, &lease.changes).await?;
    assert!(repo.manifest(&tenant, lease.job_id).await?.is_none());
    // Process loss after a verified upload: no manifest commit, then lease expiry.
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
    let reference = fleetiq_archive::objects::ArchiveObjectRef::new(
        tenant.clone(),
        stored.digest(),
        u64::try_from(stored.bytes())?,
    )?;
    let exact_bytes = objects.get(&tenant, &reference).await?;
    fixture.pause();
    let interrupted = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        fleetiq_server::archive_once(pool.clone(), fixture.config(), &tenant),
    )
    .await;
    fixture.resume();
    assert!(interrupted.is_err());
    assert!(repo.manifest(&tenant, lease.job_id).await?.is_none());
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
    // Corrupt existing bytes must fail verification without publishing or overwriting.
    assert!(
        fixture
            .raw(
                reqwest::Method::PUT,
                reference.key(),
                vec![0; exact_bytes.len()]
            )
            .await?
            .status()
            .is_success()
    );
    assert!(
        fleetiq_server::archive_once(pool.clone(), fixture.config(), &tenant)
            .await
            .is_err()
    );
    assert!(repo.manifest(&tenant, lease.job_id).await?.is_none());
    assert!(
        fixture
            .raw(reqwest::Method::PUT, reference.key(), exact_bytes)
            .await?
            .status()
            .is_success()
    );
    assert_eq!(
        fleetiq_server::archive_once(pool.clone(), fixture.config(), &tenant).await?,
        ArchiveTick::Published
    );
    let manifest = repo.manifest(&tenant, lease.job_id).await?.unwrap();
    assert_eq!(manifest.object, stored);
    assert!(manifest.generation > lease.generation);
    assert_eq!(manifest.members, 2);
    assert_eq!(
        fleetiq_server::archive_once(pool, fixture.config(), &tenant).await?,
        ArchiveTick::Idle
    );
    fixture.close();
    Ok(())
}
