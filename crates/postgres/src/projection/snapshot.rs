//! Existence, checkpoint, lag and immutable value are one repeatable read view.
use super::PostgresCurrentStateRepository;
use crate::{
    error::{PostgresRepositoryError, classify_database_error, corrupt},
    history::HistoryRow,
};
use fleetiq_domain::{SignalTarget, TenantId};
use fleetiq_projection::{CurrentPropertySnapshot, CurrentStateRepository};
impl CurrentStateRepository for PostgresCurrentStateRepository {
    type Error = PostgresRepositoryError;
    async fn snapshot(
        &self,
        tenant: &TenantId,
        target: &SignalTarget,
    ) -> Result<Option<CurrentPropertySnapshot>, Self::Error> {
        let mut tx = self.pool.begin().await.map_err(classify_database_error)?;
        sqlx::raw_sql("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY; SET LOCAL statement_timeout = '2s'; SET LOCAL idle_in_transaction_session_timeout = '8s';")
            .execute(&mut *tx).await.map_err(classify_database_error)?;
        let version = i64::from(target.property().version().get());
        let exists = sqlx::query_file!(
            "src/projection/scope.sql",
            tenant.as_str(),
            target.asset_id().as_str(),
            target.property().id().as_str(),
            version
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        if !exists.exists {
            return Ok(None);
        }
        let head = sqlx::query_file!("src/projection/head.sql", tenant.as_str())
            .fetch_one(&mut *tx)
            .await
            .map_err(classify_database_error)?;
        let item = sqlx::query_file_as!(
            HistoryRow,
            "src/projection/current.sql",
            tenant.as_str(),
            target.asset_id().as_str(),
            target.property().id().as_str(),
            version
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        tx.commit().await.map_err(classify_database_error)?;
        let number = |n| u64::try_from(n).map_err(|e| corrupt("projection.head", e));
        Ok(Some(CurrentPropertySnapshot::new(
            number(head.generation)?,
            number(head.revision)?,
            number(head.processed_count)?,
            head.pending,
            item.map(|r| r.into_domain(tenant)).transpose()?,
        )))
    }
}
