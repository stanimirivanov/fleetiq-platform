//! One read-only repeatable snapshot across every set-based graph frontier.
mod queries;
use crate::error::{PostgresRepositoryError, classify_database_error};
use fleetiq_domain::TenantId;
use fleetiq_metadata::{SemanticGraphQuery, SemanticGraphRepository};
use sqlx::{PgPool, Postgres, Transaction};

/// Consistent PostgreSQL adapter for bounded semantic exploration.
#[derive(Clone, Debug)]
pub struct PostgresSemanticGraphRepository {
    pool: PgPool,
}
impl PostgresSemanticGraphRepository {
    /// Uses a pool connected to the fully migrated metadata schema.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
/// Private-transaction read session; drop/cancellation rolls back without writes.
///
/// Every statement has a two-second timeout. Snapshot and cutoffs are shared
/// across root lookup, relationship/binding frontiers and final asset loading.
pub struct PostgresSemanticGraphRead {
    transaction: Transaction<'static, Postgres>,
    tenant: TenantId,
    query: SemanticGraphQuery,
}
impl SemanticGraphRepository for PostgresSemanticGraphRepository {
    type Error = PostgresRepositoryError;
    type Read = PostgresSemanticGraphRead;
    async fn open(
        &self,
        tenant: &TenantId,
        query: &SemanticGraphQuery,
    ) -> Result<Self::Read, Self::Error> {
        let mut transaction = self.pool.begin().await.map_err(classify_database_error)?;
        // Settings are transaction-local: no pooled-session policy survives a
        // successful request, early return, timeout or cancellation.
        sqlx::raw_sql("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY; SET LOCAL statement_timeout = '2s'; SET LOCAL idle_in_transaction_session_timeout = '8s';")
            .execute(&mut *transaction).await.map_err(classify_database_error)?;
        Ok(PostgresSemanticGraphRead {
            transaction,
            tenant: tenant.clone(),
            query: query.clone(),
        })
    }
}
