//! Durable export selections preserve original exact evidence across restart and cancellation.
use fleetiq_domain::{
    AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, SignalTarget, TenantId,
};
use fleetiq_history::exports::{
    ExportClaim, ExportFailure, ExportKey, ExportRepository, ExportRequest, ExportStatus,
    ExportTick,
};
use fleetiq_postgres::PostgresExports;
use sqlx::PgPool;
fn request() -> ExportRequest {
    ExportRequest::new(
        TenantId::new("tenant-a").unwrap(),
        SignalTarget::new(
            AssetId::new("asset-a").unwrap(),
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new("electrical.voltage").unwrap(),
                DefinitionVersion::new(1).unwrap(),
            ),
        ),
        0,
        3000,
        None,
    )
    .unwrap()
}
async fn setup(pool: &PgPool) {
    sqlx::raw_sql(include_str!(
        "../../postgres/tests/property_history/topology.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES('tenant-a',1,'operator-a',7,30,2,16777216)").execute(pool).await.unwrap();
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn restart_and_exact_retry_preserve_selection_and_original_values(pool: PgPool) {
    setup(&pool).await;
    let req = request();
    let key = ExportKey::new("export-one").unwrap();
    let ticket = fleetiq_server::request_export(pool.clone(), &key, &req)
        .await
        .unwrap();
    let repo = PostgresExports::new(pool.clone());
    assert_eq!(
        repo.status(req.tenant(), ticket).await.unwrap(),
        ExportStatus::Pending
    );
    let ExportClaim::Claimed(old) = repo.claim(req.tenant(), ticket).await.unwrap() else {
        panic!("claim")
    };
    sqlx::query("UPDATE history.export_jobs SET lease_until=clock_timestamp()-interval '1 second' WHERE job_id=$1").bind(ticket.id()).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO history.asset_property_changes VALUES('tenant-a','message-1','vendor.protocol','1',1,'binding-1',1,'device_recorded',500,500,150)").execute(&pool).await.unwrap();
    assert_eq!(
        fleetiq_server::request_export(pool.clone(), &key, &req)
            .await
            .unwrap(),
        ticket
    );
    assert_eq!(
        fleetiq_server::export_once(pool.clone(), req.tenant(), ticket)
            .await
            .unwrap(),
        ExportTick::Ready
    );
    assert!(!repo.complete(&old, &old.payload).await.unwrap());
    let bytes = repo.artifact(req.tenant(), ticket).await.unwrap().unwrap();
    assert_eq!(bytes.format_version(), 1);
    assert_eq!(bytes.selection_generation(), 1);
    assert!(bytes.expires_at_ms() > 0);
    let rows = fleetiq_archive::decode(req.tenant(), bytes.bytes()).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].observation().value(),
        &fleetiq_domain::PropertyValue::Integer(i64::MAX)
    );
    assert!(rows.iter().all(|r| r.event_at().get() != 500));
    assert!(
        repo.artifact(&TenantId::new("tenant-b").unwrap(), ticket)
            .await
            .unwrap()
            .is_none()
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_jobs_cancellation_and_expiry_keep_capacity_conservative(pool: PgPool) {
    setup(&pool).await;
    sqlx::query("INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES('tenant-a',2,'operator-a',7,30,1,4194304)").execute(&pool).await.unwrap();
    let req = request();
    let key = ExportKey::new("first").unwrap();
    let other = ExportKey::new("second").unwrap();
    let (a, b) = tokio::join!(
        fleetiq_server::request_export(pool.clone(), &key, &req),
        fleetiq_server::request_export(pool.clone(), &other, &req)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let ticket = a.or(b).unwrap();
    let repo = PostgresExports::new(pool.clone());
    let ExportClaim::Claimed(lease) = repo.claim(req.tenant(), ticket).await.unwrap() else {
        panic!("claim")
    };
    repo.cancel(req.tenant(), ticket).await.unwrap();
    assert!(repo.artifact(req.tenant(), ticket).await.unwrap().is_none());
    assert!(matches!(
        fleetiq_server::request_export(pool.clone(), &ExportKey::new("third").unwrap(), &req).await,
        Err(ExportFailure::Denied)
    ));
    assert!(!repo.complete(&lease, &lease.payload).await.unwrap());
    repo.abandon(&lease).await.unwrap();
    let next =
        fleetiq_server::request_export(pool.clone(), &ExportKey::new("third").unwrap(), &req)
            .await
            .unwrap();
    assert_eq!(
        fleetiq_server::export_once(pool.clone(), req.tenant(), next)
            .await
            .unwrap(),
        ExportTick::Ready
    );
    sqlx::query("UPDATE history.export_jobs SET expires_at=clock_timestamp()-interval '1 second' WHERE job_id=$1").bind(next.id()).execute(&pool).await.unwrap();
    assert!(repo.artifact(req.tenant(), next).await.unwrap().is_none());
    repo.expire(req.tenant()).await.unwrap();
    assert_eq!(
        repo.status(req.tenant(), next).await.unwrap(),
        ExportStatus::Expired
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM history.export_jobs WHERE payload IS NOT NULL"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn overflow_and_disabled_policy_never_publish_partial_exports(pool: PgPool) {
    setup(&pool).await;
    let req = request();
    let key = ExportKey::new("overflow").unwrap();
    sqlx::query("INSERT INTO history.asset_property_changes SELECT tenant_id,message_id,protocol_package_id,protocol_package_version,n,'binding-1',1,'device_recorded',CASE WHEN n=1 THEN 500 ELSE 1000 END,1000,150 FROM ingestion.accepted_messages CROSS JOIN generate_series(1,15) n").execute(&pool).await.unwrap();
    assert!(matches!(
        fleetiq_server::request_export(pool.clone(), &key, &req).await,
        Err(ExportFailure::Limit)
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.export_jobs")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.work_reservations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn request_identity_terminal_state_and_current_policy_are_fenced(pool: PgPool) {
    setup(&pool).await;
    let req = request();
    let key = ExportKey::new("identity").unwrap();
    let repo = PostgresExports::new(pool.clone());
    let ticket = fleetiq_server::request_export(pool.clone(), &key, &req)
        .await
        .unwrap();
    let other = ExportRequest::new(
        req.tenant().clone(),
        req.query().target().clone(),
        0,
        4000,
        None,
    )
    .unwrap();
    assert_eq!(
        fleetiq_server::request_export(pool.clone(), &key, &other).await,
        Err(ExportFailure::Conflict)
    );
    assert!(
        sqlx::query("UPDATE history.export_jobs SET start_ms=1 WHERE job_id=$1")
            .bind(ticket.id())
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE history.export_jobs SET payload='forged'::bytea WHERE job_id=$1")
            .bind(ticket.id())
            .execute(&pool)
            .await
            .is_err()
    );
    let ExportClaim::Claimed(lease) = repo.claim(req.tenant(), ticket).await.unwrap() else {
        panic!("claim")
    };
    sqlx::query("INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id) VALUES('tenant-a',2,'operator-a')").execute(&pool).await.unwrap();
    assert!(!repo.complete(&lease, &lease.payload).await.unwrap());
    repo.abandon(&lease).await.unwrap();
    repo.cancel(req.tenant(), ticket).await.unwrap();
    assert_eq!(
        fleetiq_server::request_export(pool.clone(), &key, &req)
            .await
            .unwrap(),
        ticket
    );
    assert_eq!(
        repo.status(req.tenant(), ticket).await.unwrap(),
        ExportStatus::Cancelled
    );
    assert!(repo.artifact(req.tenant(), ticket).await.unwrap().is_none());
    assert!(
        sqlx::query("UPDATE history.export_jobs SET status='pending' WHERE job_id=$1")
            .bind(ticket.id())
            .execute(&pool)
            .await
            .is_err()
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn byte_overflow_and_default_off_admission_leave_no_partial_job(pool: PgPool) {
    use fleetiq_history::exports::ExportCodec;
    struct TooLarge;
    impl ExportCodec for TooLarge {
        fn encode(
            &self,
            _: &TenantId,
            _: &[fleetiq_history::StoredPropertyChange],
        ) -> Result<Vec<u8>, ExportFailure> {
            Ok(vec![0; 2_097_153])
        }
        fn decode(
            &self,
            _: &TenantId,
            _: &[u8],
        ) -> Result<Vec<fleetiq_history::StoredPropertyChange>, ExportFailure> {
            Err(ExportFailure::Coverage)
        }
    }
    setup(&pool).await;
    let req = request();
    let repo = PostgresExports::new(pool.clone());
    let key = ExportKey::new("bytes").unwrap();
    assert_eq!(
        repo.request(&key, &req, &TooLarge).await,
        Err(ExportFailure::Limit)
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.work_reservations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    sqlx::query("INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id) VALUES('tenant-a',2,'operator-a')").execute(&pool).await.unwrap();
    assert_eq!(
        fleetiq_server::request_export(pool.clone(), &key, &req).await,
        Err(ExportFailure::Disabled)
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.export_jobs")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn selected_restored_membership_survives_staging_expiry_and_deduplicates_hot(pool: PgPool) {
    use fleetiq_history::{
        archive_work::{ArchiveClaim, ArchiveObject, ArchiveWorkRepository},
        restore::{RestoreClaim, RestoreRepository, RestoredSegment},
    };
    use sha2::{Digest, Sha256};
    setup(&pool).await;
    let req = request();
    let archive = fleetiq_postgres::PostgresArchiveWork::new(pool.clone());
    let ArchiveClaim::Claimed(lease) = archive.claim(req.tenant()).await.unwrap() else {
        panic!("archive")
    };
    let bytes = fleetiq_archive::encode(req.tenant(), &lease.changes).unwrap();
    let object = ArchiveObject::new(
        Sha256::digest(&bytes).into(),
        i64::try_from(bytes.len()).unwrap(),
    )
    .unwrap();
    assert!(archive.complete(&lease, object).await.unwrap());
    let segment = RestoredSegment::new(bytes, lease.changes).unwrap();
    let restores = fleetiq_postgres::PostgresRestore::new(pool.clone());
    let restore = restores
        .request(req.tenant(), lease.job_id, 0)
        .await
        .unwrap();
    let selected = ExportRequest::new(
        req.tenant().clone(),
        req.query().target().clone(),
        0,
        3000,
        Some(restore),
    )
    .unwrap();
    let key = ExportKey::new("selected").unwrap();
    assert_eq!(
        fleetiq_server::request_export(pool.clone(), &key, &selected).await,
        Err(ExportFailure::Coverage)
    );
    let RestoreClaim::Claimed(attempt) = restores.claim(req.tenant(), restore).await.unwrap()
    else {
        panic!("restore")
    };
    assert!(restores.complete(&attempt, &segment).await.unwrap());
    sqlx::query("INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES('tenant-a',2,'operator-a',7,30,10,4194304)").execute(&pool).await.unwrap();
    assert_eq!(
        fleetiq_server::request_export(pool.clone(), &key, &selected).await,
        Err(ExportFailure::Denied)
    );
    sqlx::query("INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES('tenant-a',3,'operator-a',7,30,10,16777216)").execute(&pool).await.unwrap();
    let ticket = fleetiq_server::request_export(pool.clone(), &key, &selected)
        .await
        .unwrap();
    sqlx::query("UPDATE history.restore_jobs SET expires_at=clock_timestamp()-interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        fleetiq_server::export_once(pool.clone(), req.tenant(), ticket)
            .await
            .unwrap(),
        ExportTick::Ready
    );
    let bytes = PostgresExports::new(pool.clone())
        .artifact(req.tenant(), ticket)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fleetiq_archive::decode(req.tenant(), bytes.bytes()).unwrap(),
        segment.changes()
    );
    assert_eq!(
        fleetiq_server::request_export(pool.clone(), &key, &selected)
            .await
            .unwrap(),
        ticket
    );
    let other = ExportKey::new("later-selection").unwrap();
    assert_eq!(
        fleetiq_server::request_export(pool.clone(), &other, &selected).await,
        Err(ExportFailure::Coverage)
    );
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn cancellation_cannot_clear_a_running_lease_in_the_same_transition(pool: PgPool) {
    setup(&pool).await;
    let req = request();
    let ticket = fleetiq_server::request_export(
        pool.clone(),
        &ExportKey::new("cancel-fence").unwrap(),
        &req,
    )
    .await
    .unwrap();
    let repo = PostgresExports::new(pool.clone());
    let ExportClaim::Claimed(_) = repo.claim(req.tenant(), ticket).await.unwrap() else {
        panic!("claim")
    };
    assert!(sqlx::query("UPDATE history.export_jobs SET status='cancelled',payload=NULL,lease_until=NULL WHERE job_id=$1").bind(ticket.id()).execute(&pool).await.is_err());
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn publication_cannot_commit_after_its_attempt_deadline(pool: PgPool) {
    setup(&pool).await;
    let req = request();
    let ticket =
        fleetiq_server::request_export(pool.clone(), &ExportKey::new("deadline").unwrap(), &req)
            .await
            .unwrap();
    let repo = PostgresExports::new(pool.clone());
    let ExportClaim::Claimed(_) = repo.claim(req.tenant(), ticket).await.unwrap() else {
        panic!("claim")
    };
    let mut tx = pool.begin().await.unwrap();
    let deadline:i64=sqlx::query_scalar("UPDATE history.export_jobs SET lease_until=clock_timestamp()+interval '3 seconds' WHERE job_id=$1 RETURNING floor(extract(epoch FROM lease_until)*1000)::bigint").bind(ticket.id()).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("UPDATE history.export_jobs SET status='ready',lease_until=NULL WHERE job_id=$1")
        .bind(ticket.id())
        .execute(&mut *tx)
        .await
        .unwrap();
    // Observe the database deadline rather than assuming a wall-clock sleep elapsed.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let expired: bool = sqlx::query_scalar(
                "SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint > $1",
            )
            .bind(deadline)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
            if expired {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(tx.commit().await.is_err());
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn each_creation_consumes_one_distinct_preflight_reservation(pool: PgPool) {
    setup(&pool).await;
    let req = request();
    let ticket =
        fleetiq_server::request_export(pool.clone(), &ExportKey::new("template").unwrap(), &req)
            .await
            .unwrap();
    sqlx::query("INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES('tenant-a',2,'operator-a',7,30,10,67108864)").execute(&pool).await.unwrap();
    let budgets = fleetiq_postgres::PostgresHistoryBudgets::new(pool.clone());
    let mut rejected = Vec::new();
    for (consume, copies) in [(false, 1), (true, 2)] {
        let fleetiq_history::lifecycle::BudgetAdmission::Reserved(res) =
            budgets.reserve(req.tenant(), 4_194_304).await.unwrap()
        else {
            panic!("reserve")
        };
        let mut tx = pool.begin().await.unwrap();
        let mut failed = false;
        for n in 0..copies {
            let key = format!("direct-{consume}-{n}");
            failed=sqlx::query("INSERT INTO history.export_jobs(tenant_id,request_key,asset_id,property_id,property_version,start_ms,end_ms,manifest_id,restore_generation,row_count,payload,fingerprint,file_bytes,admitted_by) SELECT tenant_id,$2,asset_id,property_id,property_version,start_ms,end_ms,manifest_id,restore_generation,row_count,payload,fingerprint,file_bytes,$3 FROM history.export_jobs WHERE job_id=$1").bind(ticket.id()).bind(key).bind(res).execute(&mut *tx).await.is_err();
            if failed {
                break;
            }
        }
        if failed {
            tx.rollback().await.unwrap();
        } else {
            if consume {
                sqlx::query("DELETE FROM history.work_reservations WHERE tenant_id='tenant-a' AND reservation_id=$1").bind(res).execute(&mut *tx).await.unwrap();
            }
            failed = tx.commit().await.is_err();
        }
        rejected.push(failed);
        budgets.release(req.tenant(), res).await.unwrap();
    }
    assert_eq!(rejected, vec![true, true]);
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn exports_and_other_history_work_share_independent_slot_and_byte_limits(pool: PgPool) {
    use fleetiq_history::lifecycle::BudgetAdmission;
    setup(&pool).await;
    let req = request();
    let budgets = fleetiq_postgres::PostgresHistoryBudgets::new(pool.clone());
    let repo = PostgresExports::new(pool.clone());
    sqlx::query("INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES('tenant-a',2,'operator-a',7,30,1,16777216)").execute(&pool).await.unwrap();
    let key = ExportKey::new("slot-first").unwrap();
    let ticket = fleetiq_server::request_export(pool.clone(), &key, &req)
        .await
        .unwrap();
    assert_eq!(
        fleetiq_server::export_once(pool.clone(), req.tenant(), ticket)
            .await
            .unwrap(),
        ExportTick::Ready
    );
    assert_eq!(
        budgets.reserve(req.tenant(), 1).await.unwrap(),
        BudgetAdmission::Denied
    );
    repo.cancel(req.tenant(), ticket).await.unwrap();
    let BudgetAdmission::Reserved(res) = budgets.reserve(req.tenant(), 1).await.unwrap() else {
        panic!("reserve")
    };
    assert_eq!(
        fleetiq_server::request_export(
            pool.clone(),
            &ExportKey::new("slot-blocked").unwrap(),
            &req
        )
        .await,
        Err(ExportFailure::Denied)
    );
    budgets.release(req.tenant(), res).await.unwrap();
    sqlx::query("INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES('tenant-a',3,'operator-a',7,30,10,4194304)").execute(&pool).await.unwrap();
    let ticket =
        fleetiq_server::request_export(pool.clone(), &ExportKey::new("byte-first").unwrap(), &req)
            .await
            .unwrap();
    assert_eq!(
        budgets.reserve(req.tenant(), 1).await.unwrap(),
        BudgetAdmission::Denied
    );
    let ExportClaim::Claimed(lease) = repo.claim(req.tenant(), ticket).await.unwrap() else {
        panic!("claim")
    };
    repo.cancel(req.tenant(), ticket).await.unwrap();
    assert_eq!(
        budgets.reserve(req.tenant(), 1).await.unwrap(),
        BudgetAdmission::Denied
    );
    repo.abandon(&lease).await.unwrap();
    let BudgetAdmission::Reserved(res) = budgets.reserve(req.tenant(), 1).await.unwrap() else {
        panic!("reserve")
    };
    assert_eq!(
        fleetiq_server::request_export(
            pool.clone(),
            &ExportKey::new("byte-blocked").unwrap(),
            &req
        )
        .await,
        Err(ExportFailure::Denied)
    );
    budgets.release(req.tenant(), res).await.unwrap();
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn simultaneous_identical_requests_converge_on_one_frozen_generation(pool: PgPool) {
    setup(&pool).await;
    let req = request();
    let key = ExportKey::new("same").unwrap();
    let (a, b) = tokio::join!(
        fleetiq_server::request_export(pool.clone(), &key, &req),
        fleetiq_server::request_export(pool.clone(), &key, &req)
    );
    assert_eq!(a.unwrap(), b.unwrap());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.export_jobs")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.work_reservations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn failed_capture_returns_a_same_key_winner_that_committed_during_capture(pool: PgPool) {
    use fleetiq_history::{StoredPropertyChange, exports::ExportCodec};
    use std::sync::Mutex;
    struct FailAfterWinner {
        started: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl ExportCodec for FailAfterWinner {
        fn encode(
            &self,
            _: &TenantId,
            _: &[StoredPropertyChange],
        ) -> Result<Vec<u8>, ExportFailure> {
            self.started
                .lock()
                .unwrap()
                .take()
                .unwrap()
                .send(())
                .map_err(|()| ExportFailure::Storage)?;
            self.release
                .lock()
                .unwrap()
                .recv()
                .map_err(|_| ExportFailure::Storage)?;
            Err(ExportFailure::Limit)
        }
        fn decode(
            &self,
            _: &TenantId,
            _: &[u8],
        ) -> Result<Vec<StoredPropertyChange>, ExportFailure> {
            Err(ExportFailure::Coverage)
        }
    }
    setup(&pool).await;
    let req = request();
    let key = ExportKey::new("committed-winner").unwrap();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let codec = FailAfterWinner {
        started: Mutex::new(Some(started)),
        release: Mutex::new(wait),
    };
    let runtime = tokio::runtime::Handle::current();
    let owned_pool = pool.clone();
    let owned_req = req.clone();
    let owned_key = key.clone();
    let losing = tokio::task::spawn_blocking(move || {
        runtime.block_on(PostgresExports::new(owned_pool).request(&owned_key, &owned_req, &codec))
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    let winning = fleetiq_server::request_export(pool.clone(), &key, &req)
        .await
        .unwrap();
    release.send(()).unwrap();
    assert_eq!(losing.await.unwrap().unwrap(), winning);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history.export_jobs")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
}
