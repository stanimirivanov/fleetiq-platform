//! Explicit trusted one-turn reclamation; never activated by startup or a preview.
use fleetiq_archive::objects::{ImmutableArchiveObjects, S3ArchiveConfig};
use fleetiq_history::retention_work::{RetentionFailure, RetentionTarget, RetentionTick};
use fleetiq_postgres::PostgresHotRetention;
/// Freshly verifies one manifest then atomically rechecks its explicit approval and protections.
/// The operator must separately approve this exact policy revision in the private database.
/// # Errors
/// Returns redacted provider/codec/database/deadline failures. Retry the exact target to reconcile.
pub async fn reclaim_once(
    pool: sqlx::PgPool,
    config: S3ArchiveConfig,
    target: &RetentionTarget,
) -> Result<RetentionTick, RetentionFailure> {
    let source = ImmutableArchiveObjects::connect(config).map_err(|_| RetentionFailure)?;
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        fleetiq_history::retention_work::reclaim_once(
            &PostgresHotRetention::new(pool),
            &source,
            target,
        ),
    )
    .await
    .map_err(|_| RetentionFailure)?
}
