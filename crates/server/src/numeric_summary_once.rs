//! Explicit one-turn derived summary production; no startup worker or raw-history substitution.
use fleetiq_history::{
    downsampling::{NumericRequest, SummaryFailure, SummaryReceipt},
    lifecycle::BudgetAdmission,
};
use fleetiq_postgres::{PostgresHistoryBudgets, PostgresNumericSummaries};
/// Produces one exact versioned snapshot under current tenant work admission.
/// Trusted callers establish tenant authority. Timeout/cancellation leaves capacity until expiry.
/// # Errors
/// Returns explicit disabled/denied/coverage/context failures; uncertainty is safe to retry.
pub async fn produce_numeric_summary_once(
    pool: sqlx::PgPool,
    request: &NumericRequest,
) -> Result<SummaryReceipt, SummaryFailure> {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
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
