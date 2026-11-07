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

#[test]
fn archive_worker_rejects_missing_storage_before_connecting() {
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_fleetiq-server"))
        .arg("archive-worker")
        .env_clear()
        .env("FLEETIQ_ARCHIVE_TENANT_ID", "tenant-a")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(
        String::from_utf8(result.stderr).unwrap().trim(),
        "archive worker storage configuration required"
    );
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn archive_worker_restarts_the_same_frozen_selection(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    use sqlx::ConnectOptions;
    use std::{process::Stdio, time::Duration};
    use tokio::process::{Child, Command};

    async fn stop(child: &mut Child) -> Result<(), Box<dyn std::error::Error>> {
        let pid = child.id().ok_or("archive worker exited")?.to_string();
        assert!(
            Command::new("kill")
                .args(["-TERM", &pid])
                .status()
                .await?
                .success()
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(5), child.wait())
                .await??
                .success()
        );
        Ok(())
    }

    sqlx::raw_sql(include_str!(
        "../../postgres/tests/property_history/topology.sql"
    ))
    .execute(&pool)
    .await?;
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,1,2097152)")
        .execute(&pool).await?;
    let fixture = provider::Provider::start().await;
    let start = || {
        Command::new(env!("CARGO_BIN_EXE_fleetiq-server"))
            .arg("archive-worker")
            .env_clear()
            .env(
                "DATABASE_URL",
                pool.connect_options().to_url_lossy().as_str(),
            )
            .env("FLEETIQ_ARCHIVE_TENANT_ID", "tenant-a")
            .env("FLEETIQ_ARCHIVE_ENDPOINT", &fixture.endpoint)
            .env("FLEETIQ_ARCHIVE_BUCKET", "archive-test")
            .env("FLEETIQ_ARCHIVE_REGION", "us-east-1")
            .env("FLEETIQ_ARCHIVE_ACCESS_KEY", "archive-fixture-access")
            .env("FLEETIQ_ARCHIVE_SECRET_KEY", "fixture-secret")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
    };
    fixture.pause();
    let mut first = start()?;
    let id = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            assert!(first.try_wait()?.is_none(), "archive worker exited before claiming work");
            if let Some(id) = sqlx::query_scalar::<_, i64>("SELECT job_id FROM history.archive_jobs WHERE tenant_id='tenant-a' AND lease_until IS NOT NULL")
                .fetch_optional(&pool).await? {
                break Ok::<_, Box<dyn std::error::Error>>(id);
            }
            tokio::task::yield_now().await;
        }
    }).await??;
    stop(&mut first).await?;
    let tenant = TenantId::new("tenant-a")?;
    let repo = PostgresArchiveWork::new(pool.clone());
    assert!(repo.manifest(&tenant, id).await?.is_none());
    sqlx::raw_sql("UPDATE history.archive_jobs SET lease_until=clock_timestamp()-interval '1 second'; UPDATE history.work_reservations SET expires_at=clock_timestamp()-interval '1 second'")
        .execute(&pool).await?;
    fixture.resume();
    let mut restarted = start()?;
    let manifest = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            assert!(
                restarted.try_wait()?.is_none(),
                "archive worker exited before publication"
            );
            if let Some(manifest) = repo.manifest(&tenant, id).await? {
                break Ok::<_, Box<dyn std::error::Error>>(manifest);
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    stop(&mut restarted).await?;
    assert_eq!(manifest.members, 2);
    let (jobs, reclaimed): (i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM history.archive_jobs), (SELECT count(*) FROM history.asset_property_changes WHERE event_at_ms IS NULL)")
        .fetch_one(&pool).await?;
    assert_eq!((jobs, reclaimed), (1, 0));
    fixture.close();
    Ok(())
}
