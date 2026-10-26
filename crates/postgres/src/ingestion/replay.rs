//! Replay persistence: tenant/session lock precedes work/result locks.
mod claim;
mod completion;
mod enqueue;
mod row;
use crate::{PostgresDecodedObservationLogError, PostgresRepositoryError};
use fleetiq_ingestion::{DecodedObservationBatch, ReplayLease, ReplayRepository, ReplayWork};
use sqlx::PgPool;
/// PostgreSQL-backed durable protocol replay and atomic checkpoint completion.
#[derive(Clone, Debug)]
pub struct PostgresReplayRepository {
    pool: PgPool,
}
impl PostgresReplayRepository {
    /// Uses an existing runtime pool; does not migrate or spawn a worker.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
impl ReplayRepository for PostgresReplayRepository {
    type Error = PostgresReplayError;
    async fn enqueue(&self, work: &ReplayWork) -> Result<bool, Self::Error> {
        self.enqueue_work(work).await
    }
    async fn claim(&self) -> Result<Option<ReplayLease>, Self::Error> {
        self.claim_work().await
    }
    async fn complete(
        &self,
        lease: &ReplayLease,
        batch: &DecodedObservationBatch,
        state: &[u8],
    ) -> Result<bool, Self::Error> {
        self.finish(lease, batch, state).await
    }
    async fn poison(&self, lease: &ReplayLease) -> Result<bool, Self::Error> {
        let result = sqlx::query!(
            r#"
            UPDATE ingestion.replay_work
            SET status='poisoned'
            WHERE tenant_id=$1
            AND message_id=$2
            AND generation=$3
            AND status='pending'
            AND lease_until_ms>floor(extract(epoch FROM clock_timestamp())*1000)::bigint
            "#,
            lease.work.message.tenant_id().as_str(),
            lease.work.message.id().as_str(),
            lease.generation
        )
        .execute(&self.pool)
        .await
        .map_err(crate::error::classify_database_error)?;
        Ok(result.rows_affected() == 1)
    }
}
/// Typed failures preserve storage causes without exposing replay payloads.
#[derive(Debug)]
pub enum PostgresReplayError {
    /// Database failure or invalid reconstructed state.
    Repository(PostgresRepositoryError),
    /// Conflicting or unavailable decoded-result append.
    Decoded(PostgresDecodedObservationLogError),
    /// Invalid replay input supplied by the trusted caller.
    Invalid,
}
impl From<PostgresRepositoryError> for PostgresReplayError {
    fn from(e: PostgresRepositoryError) -> Self {
        Self::Repository(e)
    }
}
impl From<PostgresDecodedObservationLogError> for PostgresReplayError {
    fn from(e: PostgresDecodedObservationLogError) -> Self {
        Self::Decoded(e)
    }
}
impl std::fmt::Display for PostgresReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("durable replay operation failed")
    }
}
impl std::error::Error for PostgresReplayError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Repository(e) => Some(e),
            Self::Decoded(e) => Some(e),
            Self::Invalid => None,
        }
    }
}
