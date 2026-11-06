//! Real provider restore is exact, atomic, bounded and independently expiring.
#[path = "../../archive/tests/support/provider.rs"]
mod provider;
use fleetiq_archive::objects::{ArchiveObjectRef, ImmutableArchiveObjects};
use fleetiq_domain::TenantId;
use fleetiq_history::{
    archive_work::{ArchiveClaim, ArchiveSegmentStore, ArchiveWorkRepository},
    restore::{RestoreStatus, RestoreTick},
};
use fleetiq_postgres::{PostgresArchiveWork, PostgresRestore};
use sqlx::PgPool;
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn restore_rejects_corruption_and_recovers_exact_evidence(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::raw_sql(include_str!(
        "../../postgres/tests/property_history/topology.sql"
    ))
    .execute(&pool)
    .await?;
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,2,4194304)").execute(&pool).await?;
    let fixture = provider::Provider::start().await;
    let tenant = TenantId::new("tenant-a")?;
    let archive = PostgresArchiveWork::new(pool.clone());
    let objects = ImmutableArchiveObjects::connect(fixture.config())?;
    let ArchiveClaim::Claimed(selection) = archive.claim(&tenant).await? else {
        panic!("selection expected")
    };
    let object = objects.store(&tenant, &selection.changes).await?;
    let restores = PostgresRestore::new(pool.clone());
    assert!(
        restores
            .request(&tenant, selection.job_id, 0)
            .await
            .is_err()
    );
    assert!(archive.complete(&selection, object).await?);
    let ticket = restores.request(&tenant, selection.job_id, 0).await?;
    assert_eq!(
        restores.request(&tenant, selection.job_id, 0).await?,
        ticket
    );
    assert_eq!(
        restores.status(&tenant, &ticket).await?,
        RestoreStatus::Pending
    );
    assert!(restores.staged(&tenant, &ticket).await?.is_none());
    let reference = ArchiveObjectRef::new(
        tenant.clone(),
        object.digest(),
        u64::try_from(object.bytes())?,
    )?;
    let bytes = objects.get(&tenant, &reference).await?;
    assert!(
        fixture
            .raw(reqwest::Method::PUT, reference.key(), vec![0; bytes.len()])
            .await?
            .status()
            .is_success()
    );
    assert!(
        fleetiq_server::restore_once(pool.clone(), fixture.config(), &tenant, &ticket)
            .await
            .is_err()
    );
    assert!(restores.staged(&tenant, &ticket).await?.is_none());
    assert!(
        fixture
            .raw(reqwest::Method::PUT, reference.key(), bytes.clone())
            .await?
            .status()
            .is_success()
    );
    interrupt_restore(&pool, &fixture, &tenant, &ticket).await?;
    assert_eq!(
        fleetiq_server::restore_once(pool.clone(), fixture.config(), &tenant, &ticket).await?,
        RestoreTick::Ready
    );
    let staged = restores.staged(&tenant, &ticket).await?.unwrap();
    assert_eq!(staged, bytes);
    assert_eq!(
        fleetiq_archive::decode(&tenant, &staged)?,
        selection.changes
    );
    assert!(
        restores
            .staged(&TenantId::new("tenant-b")?, &ticket)
            .await?
            .is_none()
    );
    sqlx::query("UPDATE history.restore_jobs SET expires_at=clock_timestamp()-interval '1 second'")
        .execute(&pool)
        .await?;
    assert_eq!(
        restores.status(&tenant, &ticket).await?,
        RestoreStatus::Expired
    );
    assert!(restores.staged(&tenant, &ticket).await?.is_none());
    restores.expire(&tenant).await?;
    assert_eq!(objects.get(&tenant, &reference).await?, bytes);
    assert!(archive.manifest(&tenant, selection.job_id).await?.is_some());
    let next = restores.request(&tenant, selection.job_id, 1).await?;
    assert_ne!(next, ticket);
    assert!(restores.staged(&tenant, &ticket).await?.is_none());
    fixture.close();
    Ok(())
}

async fn interrupt_restore(
    pool: &PgPool,
    fixture: &provider::Provider,
    tenant: &TenantId,
    ticket: &fleetiq_history::restore::RestoreTicket,
) -> Result<(), Box<dyn std::error::Error>> {
    fixture.pause();
    let interrupted = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        fleetiq_server::restore_once(pool.clone(), fixture.config(), tenant, ticket),
    )
    .await;
    fixture.resume();
    assert!(interrupted.is_err());
    sqlx::query(
        "UPDATE history.restore_jobs SET lease_until=clock_timestamp()-interval '1 second'",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE history.work_reservations SET expires_at=clock_timestamp()-interval '1 second'",
    )
    .execute(pool)
    .await?;
    Ok(())
}
