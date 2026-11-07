//! Explicit trusted one-turn reclamation; never activated by startup or a preview.
use crate::{SchemaMode, config::archive_from_env};
use fleetiq_domain::TenantId;
use std::{env, error::Error, time::Duration};

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
        Duration::from_secs(20),
        fleetiq_history::retention_work::reclaim_once(
            &PostgresHotRetention::new(pool),
            &source,
            target,
        ),
    )
    .await
    .map_err(|_| RetentionFailure)?
}

/// Reads one explicit retention target, validates the installed schema and runs one turn.
/// # Errors
/// Returns redacted input, startup, schema or reclamation errors.
pub async fn run_reclaim_once() -> Result<RetentionTick, Box<dyn Error>> {
    let target = (|| {
        RetentionTarget::new(
            TenantId::new(env::var("FLEETIQ_RECLAIM_TENANT_ID").ok()?).ok()?,
            env::var("FLEETIQ_RECLAIM_MANIFEST_ID").ok()?.parse().ok()?,
            env::var("FLEETIQ_RECLAIM_POLICY_REVISION")
                .ok()?
                .parse()
                .ok()?,
        )
        .ok()
    })()
    .ok_or("invalid reclaim target")?;
    let archive = archive_from_env()?.ok_or("reclaim storage configuration required")?;
    let url = env::var("DATABASE_URL").map_err(|_| "database configuration required")?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&url)
        .await
        .map_err(|_| "reclaim database unavailable")?;
    tokio::time::timeout(Duration::from_secs(5), SchemaMode::Check.prepare(&pool))
        .await
        .map_err(|_| "reclaim schema check timed out")?
        .map_err(|_| "reclaim schema incompatible")?;
    Ok(reclaim_once(pool, archive, &target).await?)
}
