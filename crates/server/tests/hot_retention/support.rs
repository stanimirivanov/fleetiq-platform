use fleetiq_domain::TenantId;
use fleetiq_history::{
    archive_work::{ArchiveClaim, ArchiveWorkRepository},
    retention_work::RetentionTarget,
};
use fleetiq_postgres::PostgresArchiveWork;
use fleetiq_projection::{ProjectionMaintenance, ProjectionTick};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
type Error = Box<dyn std::error::Error>;
pub(super) async fn selection(
    pool: &PgPool,
) -> Result<
    (
        RetentionTarget,
        Vec<fleetiq_history::StoredPropertyChange>,
        Vec<u8>,
    ),
    Error,
> {
    sqlx::raw_sql(include_str!(
        "../../../postgres/tests/property_history/topology.sql"
    ))
    .execute(pool)
    .await?;
    sqlx::query("INSERT INTO history.lifecycle_policies (tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES ('tenant-a',1,'operator-a',7,30,2,4194304)").execute(pool).await?;
    let tenant = TenantId::new("tenant-a")?;
    let ArchiveClaim::Claimed(lease) = PostgresArchiveWork::new(pool.clone())
        .claim(&tenant)
        .await?
    else {
        panic!("selection");
    };
    let bytes = fleetiq_archive::encode(&tenant, &lease.changes)?;
    let target = RetentionTarget::new(tenant, lease.job_id, 1)?;
    // A synthetic already-published historical receipt, with real exact file bytes.
    // Production never backdates a manifest. No existing published row is mutated.
    sqlx::query("UPDATE history.archive_jobs SET object_sha256=$3,object_bytes=$4,lease_until=clock_timestamp()-interval '8 days' WHERE tenant_id=$1 AND job_id=$2")
        .bind(target.tenant().as_str()).bind(target.manifest()).bind(Sha256::digest(&bytes).to_vec()).bind(i64::try_from(bytes.len())?).execute(pool).await?;
    fleetiq_postgres::PostgresHistoryBudgets::new(pool.clone())
        .release(target.tenant(), lease.reservation)
        .await?;
    Ok((target, lease.changes, bytes))
}
pub(super) async fn quiesce(pool: &PgPool) -> Result<(), Error> {
    // A later interpretation supersedes both archived rows, preserving the live winner.
    sqlx::query("INSERT INTO history.asset_property_changes VALUES ('tenant-a','message-1','vendor.protocol','1',2,'binding-1',1,'received_at',2000,1000,150)").execute(pool).await?;
    drain(pool).await
}
pub(super) async fn drain(pool: &PgPool) -> Result<(), Error> {
    let worker = fleetiq_history::SemanticWorker::new(
        fleetiq_postgres::PostgresSemanticWorkRepository::new(pool.clone()),
        fleetiq_postgres::PostgresDecodedObservationView::new(pool.clone()),
        fleetiq_postgres::PostgresSignalBindingRepository::new(pool.clone()),
        fleetiq_postgres::PostgresDefinitionRepository::new(pool.clone()),
    );
    let mut idle = false;
    for _ in 0..256 {
        if worker.run_once().await? == fleetiq_history::SemanticTick::Idle {
            idle = true;
            break;
        }
    }
    assert!(idle, "fixture work must quiesce");
    let projection = fleetiq_postgres::PostgresCurrentStateRepository::new(pool.clone());
    let mut projection_idle = false;
    for _ in 0..4 {
        if projection.tick().await? == ProjectionTick::Idle {
            projection_idle = true;
            break;
        }
    }
    assert!(projection_idle, "bounded fixture projection must quiesce");
    sqlx::query("UPDATE history.semantic_backfill SET completed=true")
        .execute(pool)
        .await?;
    Ok(())
}

pub(super) struct Source<'a>(pub &'a fleetiq_history::restore::RestoredSegment);
impl fleetiq_history::restore::ArchiveRestoreSource for Source<'_> {
    fn load(
        &self,
        _tenant: &TenantId,
        _object: fleetiq_history::archive_work::ArchiveObject,
    ) -> impl std::future::Future<
        Output = Result<
            fleetiq_history::restore::RestoredSegment,
            fleetiq_history::restore::RestoreFailure,
        >,
    > + Send {
        std::future::ready(fleetiq_history::restore::RestoredSegment::new(
            self.0.bytes().to_vec(),
            self.0.changes().to_vec(),
        ))
    }
}
