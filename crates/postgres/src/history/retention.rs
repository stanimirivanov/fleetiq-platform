//! Repeatable-read retention diagnostics, never a deletion or provider-verification boundary.
use crate::error::{PostgresRepositoryError, classify_database_error};
use fleetiq_access::{AuthorizedTenant, ReadOperations};
use fleetiq_history::lifecycle::retention::{
    RetentionCandidate, RetentionConsumer, RetentionPreview,
};
use sqlx::PgPool;
/// Tenant-scoped bounded eligibility preview under operations.read authority.
pub struct PostgresRetentionPreview(PgPool);
impl PostgresRetentionPreview {
    /// Uses an already migrated pool without starting retention work.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    /// Samples at most one hundred complete interpretation identities in one snapshot.
    /// # Errors
    /// Returns database/deadline failures; errors never become empty eligibility reports.
    pub async fn snapshot(
        &self,
        auth: AuthorizedTenant<ReadOperations>,
    ) -> Result<RetentionPreview, PostgresRepositoryError> {
        let mut tx = self.0.begin().await.map_err(classify_database_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(classify_database_error)?;
        sqlx::query("SET LOCAL statement_timeout='2s'")
            .execute(&mut *tx)
            .await
            .map_err(classify_database_error)?;
        let meta = sqlx::query_file!(
            "src/history/retention/context.sql",
            auth.tenant_id().as_str()
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        let mut rows = sqlx::query_file!(
            "src/history/retention/candidates.sql",
            auth.tenant_id().as_str()
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        tx.commit().await.map_err(classify_database_error)?;
        let has_more = rows.len() > 100;
        rows.truncate(100);
        let consumers = [
            (meta.replay, RetentionConsumer::Replay),
            (meta.semantic, RetentionConsumer::Semantic),
            (meta.calculator, RetentionConsumer::Calculator),
            (meta.restore, RetentionConsumer::Restore),
            (meta.projection, RetentionConsumer::Projection),
        ]
        .into_iter()
        .filter_map(|(held, consumer)| held.then_some(consumer))
        .collect::<Vec<_>>();
        let candidates = rows
            .into_iter()
            .map(|row| {
                let mut candidate = RetentionCandidate::assess(
                    row.id,
                    row.age_floor,
                    meta.observed,
                    meta.hot_days,
                    row.verified,
                    consumers.clone(),
                );
                for (protected, reason) in [
                    (
                        !row.payload_present,
                        fleetiq_history::lifecycle::EvidenceProtection::ArchivedPayload,
                    ),
                    (
                        row.current_projection,
                        fleetiq_history::lifecycle::EvidenceProtection::CurrentProjection,
                    ),
                    (
                        row.pinned_analytics,
                        fleetiq_history::lifecycle::EvidenceProtection::PinnedAnalytics,
                    ),
                ] {
                    if protected {
                        candidate.protections.push(reason);
                    }
                }
                candidate
            })
            .collect();
        Ok(RetentionPreview {
            observed_at_ms: meta.observed,
            policy_revision: meta.revision,
            candidates,
            has_more,
        })
    }
}
