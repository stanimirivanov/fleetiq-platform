//! Shared bounded archive turn for trusted one-shot and worker entry points.
use fleetiq_archive::objects::{ImmutableArchiveObjects, S3ArchiveConfig};
use fleetiq_domain::TenantId;
use fleetiq_history::archive_work::{ArchiveFailure, ArchiveTick};
use fleetiq_postgres::PostgresArchiveWork;
/// Archives one bounded selection using operator configuration and a migrated pool.
/// The trusted caller establishes tenant authority. Cancellation retains frozen
/// membership; the thirty-second lease/reservation expires after this twenty-second limit.
/// # Errors
/// Returns redacted configuration, storage, codec, publication or deadline failures.
pub async fn archive_once(
    pool: sqlx::PgPool,
    config: S3ArchiveConfig,
    tenant: &TenantId,
) -> Result<ArchiveTick, ArchiveFailure> {
    let store = ImmutableArchiveObjects::connect(config).map_err(|_| ArchiveFailure)?;
    archive_turn(&PostgresArchiveWork::new(pool), &store, tenant).await
}

pub(crate) async fn archive_turn(
    repository: &PostgresArchiveWork,
    store: &ImmutableArchiveObjects,
    tenant: &TenantId,
) -> Result<ArchiveTick, ArchiveFailure> {
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        fleetiq_history::archive_work::archive_once(repository, store, tenant),
    )
    .await
    .map_err(|_| ArchiveFailure)?
}
