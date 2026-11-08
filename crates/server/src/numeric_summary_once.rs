//! Explicit one-turn derived summary production; no startup worker or raw-history substitution.
use crate::SchemaMode;
use fleetiq_domain::{
    AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, SignalTarget, TenantId,
};
use fleetiq_history::{
    downsampling::{NumericRequest, NumericSpec, SummaryFailure, SummaryReceipt},
    lifecycle::BudgetAdmission,
    restore::RestoreTicket,
};
use fleetiq_postgres::{PostgresHistoryBudgets, PostgresNumericSummaries};
use std::{env, error::Error, time::Duration};
/// Produces one exact versioned snapshot under current tenant work admission.
/// Trusted callers establish tenant authority. Timeout/cancellation leaves capacity until expiry.
/// # Errors
/// Returns explicit disabled/denied/coverage/context failures; uncertainty is safe to retry.
pub async fn produce_numeric_summary_once(
    pool: sqlx::PgPool,
    request: &NumericRequest,
) -> Result<SummaryReceipt, SummaryFailure> {
    tokio::time::timeout(Duration::from_secs(20), async {
        let budgets = PostgresHistoryBudgets::new(pool.clone());
        let reservation = match budgets
            .reserve(request.tenant(), 2_097_152)
            .await
            .map_err(|_| SummaryFailure::Storage)?
        {
            BudgetAdmission::Disabled => return Err(SummaryFailure::Disabled),
            BudgetAdmission::Denied => return Err(SummaryFailure::BudgetDenied),
            BudgetAdmission::Reserved(id) => id,
        };
        let result = async {
            let repo = PostgresNumericSummaries::new(pool);
            let snapshot = repo.capture(request, reservation).await?;
            repo.publish(snapshot, &fleetiq_archive::NumericArchiveDecoder)
                .await
        }
        .await;
        budgets
            .release(request.tenant(), reservation)
            .await
            .map_err(|_| SummaryFailure::Storage)?;
        result
    })
    .await
    .map_err(|_| SummaryFailure::Storage)?
}

fn request_from_env() -> Option<NumericRequest> {
    let tenant = TenantId::new(env::var("FLEETIQ_SUMMARY_TENANT_ID").ok()?).ok()?;
    let asset = AssetId::new(env::var("FLEETIQ_SUMMARY_ASSET_ID").ok()?).ok()?;
    let property = PropertyDefinitionId::new(env::var("FLEETIQ_SUMMARY_PROPERTY_ID").ok()?).ok()?;
    let version = DefinitionVersion::new(
        env::var("FLEETIQ_SUMMARY_PROPERTY_VERSION")
            .ok()?
            .parse()
            .ok()?,
    )
    .ok()?;
    let spec = NumericSpec::new(
        env::var("FLEETIQ_SUMMARY_START_MS").ok()?.parse().ok()?,
        env::var("FLEETIQ_SUMMARY_END_MS").ok()?.parse().ok()?,
        env::var("FLEETIQ_SUMMARY_RESOLUTION_MS")
            .ok()?
            .parse()
            .ok()?,
    )
    .ok()?;
    let restore = match (
        env::var("FLEETIQ_SUMMARY_MANIFEST_ID"),
        env::var("FLEETIQ_SUMMARY_RESTORE_GENERATION"),
    ) {
        (Err(env::VarError::NotPresent), Err(env::VarError::NotPresent)) => None,
        (Ok(manifest), Ok(generation)) => {
            Some(RestoreTicket::new(manifest.parse().ok()?, generation.parse().ok()?).ok()?)
        }
        _ => return None,
    };
    Some(NumericRequest::new(
        tenant,
        SignalTarget::new(asset, PropertyDefinitionRef::new(property, version)),
        spec,
        restore,
    ))
}

/// Validates the explicit operator target and produces one summary on a migrated database.
/// # Errors
/// Returns redacted configuration, schema, admission or production failures.
pub async fn run_summarize_once() -> Result<SummaryReceipt, Box<dyn Error>> {
    let request = request_from_env().ok_or("invalid numeric summary target")?;
    let url = env::var("DATABASE_URL").map_err(|_| "database configuration required")?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&url)
        .await
        .map_err(|_| "summary database unavailable")?;
    tokio::time::timeout(Duration::from_secs(5), SchemaMode::Check.prepare(&pool))
        .await
        .map_err(|_| "summary schema check timed out")?
        .map_err(|_| "summary schema incompatible")?;
    Ok(produce_numeric_summary_once(pool, &request).await?)
}
