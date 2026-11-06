//! PostgreSQL ownership, atomic membership and staging-budget invariants.
use fleetiq_domain::TenantId;
use fleetiq_history::{
    archive_work::{ArchiveClaim, ArchiveObject, ArchiveWorkRepository},
    lifecycle::BudgetAdmission,
    restore::{RestoreClaim, RestoreRepository, RestoreStatus, RestoredSegment},
};
use fleetiq_postgres::{PostgresArchiveWork, PostgresHistoryBudgets, PostgresRestore};
use sqlx::PgPool;
// The verifying store is stubbed at this database boundary. Real Parquet/S3 equivalence
// is exercised separately by the server archive_restore integration test.
async fn fixture(
    pool: &PgPool,
) -> Result<(TenantId, i64, RestoredSegment), Box<dyn std::error::Error>> {
    sqlx::raw_sql(include_str!("property_history/topology.sql"))
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,2,4194304)").execute(pool).await?;
    let tenant = TenantId::new("tenant-a")?;
    let archive = PostgresArchiveWork::new(pool.clone());
    let ArchiveClaim::Claimed(selection) = archive.claim(&tenant).await? else {
        panic!("selection expected")
    };
    let bytes = b"verified-object-database-fixture".to_vec();
    let hash: Vec<u8> = sqlx::query_scalar("SELECT sha256($1::bytea)")
        .bind(&bytes)
        .fetch_one(pool)
        .await?;
    assert!(
        archive
            .complete(
                &selection,
                ArchiveObject::new(hash.try_into().unwrap(), i64::try_from(bytes.len())?)?
            )
            .await?
    );
    Ok((
        tenant,
        selection.job_id,
        RestoredSegment::new(bytes, selection.changes)?,
    ))
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn cancellation_and_exact_request_retry_never_reopen_stale_work(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (tenant, manifest, segment) = fixture(&pool).await?;
    assert!(RestoredSegment::new(vec![], segment.changes().to_vec()).is_err());
    assert!(RestoredSegment::new(vec![0; 2_097_153], segment.changes().to_vec()).is_err());
    assert!(RestoredSegment::new(segment.bytes().to_vec(), vec![]).is_err());
    assert!(
        RestoredSegment::new(
            segment.bytes().to_vec(),
            vec![segment.changes()[0].clone(); 17]
        )
        .is_err()
    );
    let repo = PostgresRestore::new(pool.clone());
    let ticket = repo.request(&tenant, manifest, 0).await?;
    let RestoreClaim::Claimed(lease) = repo.claim(&tenant, ticket).await? else {
        panic!("claim expected")
    };
    assert!(!repo.cancel(&TenantId::new("tenant-b")?, &ticket).await?);
    assert!(repo.cancel(&tenant, &ticket).await?);
    assert_eq!(repo.request(&tenant, manifest, 0).await?, ticket);
    assert_eq!(
        repo.status(&tenant, &ticket).await?,
        RestoreStatus::Cancelled
    );
    assert!(!repo.complete(&lease, &segment).await?);
    assert!(repo.staged(&tenant, &ticket).await?.is_none());
    let held: i64 = sqlx::query_scalar("SELECT count(*) FROM history.work_reservations")
        .fetch_one(&pool)
        .await?;
    assert_eq!(
        held, 1,
        "external cancellation cannot prematurely free in-flight work"
    );
    repo.abandon(&lease).await?;
    let next = repo.request(&tenant, manifest, 1).await?;
    assert_ne!(next, ticket);
    assert!(repo.request(&tenant, manifest, 0).await.is_err());
    assert!(!repo.cancel(&tenant, &ticket).await?);
    let RestoreClaim::Claimed(next_lease) = repo.claim(&tenant, next).await? else {
        panic!("new generation expected")
    };
    let duplicate = RestoredSegment::new(
        segment.bytes().to_vec(),
        vec![segment.changes()[0].clone(); 2],
    )?;
    assert!(
        !repo.complete(&next_lease, &duplicate).await?,
        "count alone cannot prove membership"
    );
    let corrupt = RestoredSegment::new(vec![0; segment.bytes().len()], segment.changes().to_vec())?;
    assert!(!repo.complete(&next_lease, &corrupt).await?);
    assert!(repo.staged(&tenant, &next).await?.is_none());
    assert!(!repo.complete(&lease, &segment).await?);
    assert!(repo.complete(&next_lease, &segment).await?);
    assert!(repo.complete(&next_lease, &segment).await?);
    assert_eq!(repo.staged(&tenant, &next).await?.unwrap(), segment.bytes());
    assert!(repo.staged(&tenant, &ticket).await?.is_none());
    assert!(
        repo.staged(&TenantId::new("tenant-b")?, &next)
            .await?
            .is_none()
    );
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn live_staging_and_other_work_share_the_byte_ceiling(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (tenant, manifest, segment) = fixture(&pool).await?;
    let repo = PostgresRestore::new(pool.clone());
    let ticket = repo.request(&tenant, manifest, 0).await?;
    let RestoreClaim::Claimed(lease) = repo.claim(&tenant, ticket).await? else {
        panic!("claim expected")
    };
    assert!(repo.complete(&lease, &segment).await?);
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',2,'operator-a',7,30,2,$1)").bind(i64::try_from(segment.bytes().len())?).execute(&pool).await?;
    let budget = PostgresHistoryBudgets::new(pool.clone());
    assert_eq!(budget.reserve(&tenant, 1).await?, BudgetAdmission::Denied);
    sqlx::query("UPDATE history.restore_jobs SET expires_at=clock_timestamp()-interval '1 second'")
        .execute(&pool)
        .await?;
    assert_eq!(repo.status(&tenant, &ticket).await?, RestoreStatus::Expired);
    assert!(repo.staged(&tenant, &ticket).await?.is_none());
    repo.expire(&tenant).await?;
    let payloads: i64 =
        sqlx::query_scalar("SELECT count(*) FROM history.restore_jobs WHERE payload IS NOT NULL")
            .fetch_one(&pool)
            .await?;
    assert_eq!(payloads, 0);
    let BudgetAdmission::Reserved(id) = budget.reserve(&tenant, 1).await? else {
        panic!("expired copy no longer consumes live allowance")
    };
    budget.release(&tenant, id).await?;
    assert!(
        PostgresArchiveWork::new(pool)
            .manifest(&tenant, manifest)
            .await?
            .is_some()
    );
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn competing_claims_and_policy_changes_are_fenced(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (tenant, manifest, segment) = fixture(&pool).await?;
    let repo = PostgresRestore::new(pool.clone());
    let ticket = repo.request(&tenant, manifest, 0).await?;
    let (one, two) = tokio::join!(repo.claim(&tenant, ticket), repo.claim(&tenant, ticket));
    let ((RestoreClaim::Claimed(lease), RestoreClaim::Idle)
    | (RestoreClaim::Idle, RestoreClaim::Claimed(lease))) = (one?, two?)
    else {
        panic!("one attempt must own the request")
    };
    sqlx::query(
        "UPDATE history.restore_jobs SET lease_until=clock_timestamp()-interval '1 second'",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "UPDATE history.work_reservations SET expires_at=clock_timestamp()-interval '1 second'",
    )
    .execute(&pool)
    .await?;
    let RestoreClaim::Claimed(next) = repo.claim(&tenant, ticket).await? else {
        panic!("expired attempt expected")
    };
    assert!(next.attempt > lease.attempt);
    assert!(!repo.complete(&lease, &segment).await?);
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await?;
    assert!(!repo.complete(&next, &segment).await?);
    sqlx::query("UPDATE metadata.tenants SET is_active=true WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id) VALUES ('tenant-a',2,'operator-a')").execute(&pool).await?;
    assert!(!repo.complete(&next, &segment).await?);
    assert!(repo.staged(&tenant, &ticket).await?.is_none());
    repo.abandon(&next).await?;
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn populated_upgrade_preserves_published_manifests_and_hot_evidence(
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    for migration in fleetiq_postgres::MIGRATOR.iter().filter(|m| m.version < 55) {
        sqlx::raw_sql(migration.sql.clone()).execute(&pool).await?;
    }
    sqlx::raw_sql(include_str!("property_history/topology.sql"))
        .execute(&pool)
        .await?;
    // Seed historical committed evidence without calling a newer binary's admission queries.
    let manifest: i64 = sqlx::query_scalar(
        "INSERT INTO history.archive_jobs (tenant_id) VALUES ('tenant-a') RETURNING job_id",
    )
    .fetch_one(&pool)
    .await?;
    sqlx::query("INSERT INTO history.archive_members SELECT tenant_id,$1,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis FROM history.asset_property_changes").bind(manifest).execute(&pool).await?;
    sqlx::query("UPDATE history.archive_jobs SET generation=1,lease_until=clock_timestamp(),reservation_id=1,object_sha256=sha256('historical'::bytea),object_bytes=10").execute(&pool).await?;
    sqlx::raw_sql(include_str!("../../../migrations/0055_archive_restore.sql"))
        .execute(&pool)
        .await?;
    let (hot,archives,restores):(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM history.asset_property_changes),(SELECT count(*) FROM history.archive_jobs WHERE object_sha256 IS NOT NULL),(SELECT count(*) FROM history.restore_jobs)").fetch_one(&pool).await?;
    assert_eq!((hot, archives, restores), (2, 1, 0));
    Ok(())
}
