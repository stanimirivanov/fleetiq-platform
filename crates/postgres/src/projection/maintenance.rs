//! One transaction publishes winner pointers, pending completion and revision.
use super::PostgresCurrentStateRepository;
use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};
use fleetiq_domain::TenantId;
use fleetiq_projection::{ProjectionMaintenance, ProjectionTick};
impl ProjectionMaintenance for PostgresCurrentStateRepository {
    type Error = PostgresRepositoryError;
    async fn tick(&self) -> Result<ProjectionTick, Self::Error> {
        let mut tx = self.pool.begin().await.map_err(classify_database_error)?;
        sqlx::query("SET LOCAL statement_timeout = '5s'")
            .execute(&mut *tx)
            .await
            .map_err(classify_database_error)?;
        let row = sqlx::query_file!("src/projection/tick.sql")
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify_database_error)?;
        tx.commit().await.map_err(classify_database_error)?;
        row.map_or(Ok(ProjectionTick::Idle), |row| {
            let count = u16::try_from(row.applied_count)
                .map_err(|e| corrupt("projection.applied_count", e))?;
            if !(1..=100).contains(&count) {
                return Err(corrupt("projection.applied_count", "invalid batch"));
            }
            Ok(ProjectionTick::Applied(count))
        })
    }
    async fn rebuild(&self, tenant: &TenantId) -> Result<(), Self::Error> {
        let mut tx = self.pool.begin().await.map_err(classify_database_error)?;
        // Establish the view before any read. A conflicting head update may
        // abort with serialization failure; no half-built generation is visible.
        sqlx::raw_sql(
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ; SET LOCAL statement_timeout = '5s';",
        )
        .execute(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        sqlx::query_file!("src/projection/rebuild.sql", tenant.as_str())
            .fetch_one(&mut *tx)
            .await
            .map_err(classify_database_error)?;
        tx.commit().await.map_err(classify_database_error)
    }
}
