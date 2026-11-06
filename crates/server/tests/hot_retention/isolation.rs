//! Stale snapshots, dependency writers and expired capacity never authorize removal.
use super::*;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn stale_snapshot_cannot_authorize_a_retention_receipt(pool: PgPool) -> Result<(), Error> {
    let (target, _, _) = selection(&pool).await?;
    quiesce(&pool).await?;
    PostgresHotRetention::new(pool.clone())
        .approve(target.tenant(), 1, "operator-a")
        .await?;
    let mut stale = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *stale)
        .await?;
    sqlx::query("SELECT count(*) FROM history.restore_jobs")
        .execute(&mut *stale)
        .await?;
    fleetiq_postgres::PostgresRestore::new(pool.clone())
        .request(target.tenant(), target.manifest(), 0)
        .await?;
    let error=sqlx::query("INSERT INTO history.retention_receipts(tenant_id,manifest_id,policy_revision,actor_id,records) VALUES ('tenant-a',$1,1,'operator-a',2)").bind(target.manifest()).execute(&mut *stale).await.unwrap_err();
    assert!(error.to_string().contains("read committed"), "{error}");
    stale.rollback().await?;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn stale_snapshot_cannot_create_calculator_or_projection_references(
    pool: PgPool,
) -> Result<(), Error> {
    let (target, original, bytes) = selection(&pool).await?;
    quiesce(&pool).await?;
    let repo = PostgresHotRetention::new(pool.clone());
    repo.approve(target.tenant(), 1, "operator-a").await?;
    let key = calculator_key(&pool).await?;
    let mut stale = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *stale)
        .await?;
    sqlx::query("SELECT count(*) FROM history.asset_property_changes")
        .execute(&mut *stale)
        .await?;
    let mut projection_stale = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *projection_stale)
        .await?;
    sqlx::query("SELECT count(*) FROM history.asset_property_changes")
        .execute(&mut *projection_stale)
        .await?;
    assert_eq!(
        reclaim(&repo, &target, &RestoredSegment::new(bytes, original)?).await?,
        RetentionTick::Reclaimed(2)
    );
    let error=sqlx::query("INSERT INTO analytics.evaluations(tenant_id,evaluation_id,replay_key,key_digest,minimum_samples,evaluated_at_ms,outcome,outputs) VALUES ('tenant-a','stale-evaluation',$1,sha256(convert_to($1::jsonb::text,'UTF8')),2,3500,'insufficient','[]')").bind(key).execute(&mut *stale).await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("40001")
    );
    stale.rollback().await?;
    let error=sqlx::query("UPDATE projection.current_properties SET output_index=0,event_time_basis='device_recorded',event_at_ms=1000 WHERE tenant_id='tenant-a'").execute(&mut *projection_stale).await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("40001")
    );
    projection_stale.rollback().await?;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn executor_pins_isolation_even_when_pool_defaults_to_repeatable_read(
    pool: PgPool,
) -> Result<(), Error> {
    let (target, original, bytes) = selection(&pool).await?;
    quiesce(&pool).await?;
    let repeatable = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET default_transaction_isolation='repeatable read'")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(pool.connect_options().as_ref().clone())
        .await?;
    let repo = PostgresHotRetention::new(repeatable.clone());
    repo.approve(target.tenant(), 1, "operator-a").await?;
    assert_eq!(
        reclaim(&repo, &target, &RestoredSegment::new(bytes, original)?).await?,
        RetentionTick::Reclaimed(2)
    );
    repeatable.close().await;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn pending_projection_updates_cannot_target_reclaimed_anchors(
    pool: PgPool,
) -> Result<(), Error> {
    let (target, original, bytes) = selection(&pool).await?;
    quiesce(&pool).await?;
    let repo = PostgresHotRetention::new(pool.clone());
    repo.approve(target.tenant(), 1, "operator-a").await?;
    assert_eq!(
        reclaim(&repo, &target, &RestoredSegment::new(bytes, original)?).await?,
        RetentionTick::Reclaimed(2)
    );
    sqlx::query("INSERT INTO history.asset_property_changes VALUES ('tenant-a','message-1','vendor.protocol','1',3,'binding-1',1,'received_at',2000,1000,150)").execute(&pool).await?;
    let error=sqlx::query("UPDATE projection.pending_changes SET output_index=0,event_time_basis='device_recorded' WHERE tenant_id='tenant-a' AND output_index=3").execute(&pool).await.unwrap_err();
    assert!(
        error.to_string().contains("hot original evidence"),
        "{error}"
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn stale_preparations_cannot_persist_after_reclamation(pool: PgPool) -> Result<(), Error> {
    let (target, original, bytes) = selection(&pool).await?;
    quiesce(&pool).await?;
    let repo = PostgresHotRetention::new(pool.clone());
    repo.approve(target.tenant(), 1, "operator-a").await?;
    let mut stale = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *stale)
        .await?;
    sqlx::query("SELECT count(*) FROM history.asset_property_changes")
        .execute(&mut *stale)
        .await?;
    assert_eq!(
        reclaim(&repo, &target, &RestoredSegment::new(bytes, original)?).await?,
        RetentionTick::Reclaimed(2)
    );
    let error=sqlx::query("INSERT INTO analytics.prepared_evaluations VALUES ('tenant-a','stale-preparation','{}','{}')").execute(&mut *stale).await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("25001")
    );
    stale.rollback().await?;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn expired_reservations_never_authorize_verified_removal(pool: PgPool) -> Result<(), Error> {
    let (target, original, bytes) = selection(&pool).await?;
    quiesce(&pool).await?;
    let repo = PostgresHotRetention::new(pool.clone());
    repo.approve(target.tenant(), 1, "operator-a").await?;
    let segment = RestoredSegment::new(bytes, original)?;
    let source = ExpiringSource {
        pool: pool.clone(),
        segment: &segment,
    };
    assert_eq!(
        repo.reclaim(&target, &source).await?,
        RetentionTick::Protected
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.retention_receipts")
            .fetch_one(&pool)
            .await?,
        0
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_repeatable_read_admission_cannot_exceed_one_slot(
    pool: PgPool,
) -> Result<(), Error> {
    sqlx::raw_sql("INSERT INTO metadata.tenants VALUES ('tenant-a'); INSERT INTO identity.actors VALUES ('operator-a'); INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,1,2097152)").execute(&pool).await?;
    let repeatable = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .min_connections(2)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET default_transaction_isolation='repeatable read'")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(pool.connect_options().as_ref().clone())
        .await?;
    let mut gate = pool.begin().await?;
    sqlx::query("SELECT tenant_id FROM metadata.tenants WHERE tenant_id='tenant-a' FOR UPDATE")
        .execute(&mut *gate)
        .await?;
    let mut tasks = Vec::new();
    for _ in 0..2 {
        let p = repeatable.clone();
        tasks.push(tokio::spawn(async move {
            fleetiq_postgres::PostgresHistoryBudgets::new(p)
                .reserve(
                    &fleetiq_domain::TenantId::new("tenant-a").unwrap(),
                    2_097_152,
                )
                .await
        }));
    }
    // Observe both real database lock waits before releasing the first snapshot gate.
    let observed=tokio::time::timeout(std::time::Duration::from_secs(1),async {
        loop {
            let waiting:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE '%FROM metadata.tenants WHERE tenant_id=$1 FOR UPDATE%'").fetch_one(&pool).await?;
            if waiting==2 {return Ok::<(),sqlx::Error>(());}
            tokio::task::yield_now().await;
        }
    }).await;
    gate.commit().await?;
    let mut admitted = 0;
    for task in tasks {
        if matches!(
            task.await??,
            fleetiq_history::lifecycle::BudgetAdmission::Reserved(_)
        ) {
            admitted += 1;
        }
    }
    repeatable.close().await;
    observed??;
    assert_eq!(
        admitted, 1,
        "a row lock must not preserve a stale admission snapshot"
    );
    Ok(())
}

struct ExpiringSource<'a> {
    pool: PgPool,
    segment: &'a RestoredSegment,
}
impl fleetiq_history::restore::ArchiveRestoreSource for ExpiringSource<'_> {
    async fn load(
        &self,
        _tenant: &fleetiq_domain::TenantId,
        _object: ArchiveObject,
    ) -> Result<RestoredSegment, fleetiq_history::restore::RestoreFailure> {
        sqlx::query(
            "UPDATE history.work_reservations SET expires_at=clock_timestamp()-interval '1 second'",
        )
        .execute(&self.pool)
        .await
        .map_err(|_| fleetiq_history::restore::RestoreFailure)?;
        RestoredSegment::new(
            self.segment.bytes().to_vec(),
            self.segment.changes().to_vec(),
        )
    }
}
