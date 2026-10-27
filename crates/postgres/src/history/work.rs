//! History-owned scheduling adapter; tenant lock always precedes work lock.
mod claim;
mod completion;
mod seed;
use crate::{PostgresAssetPropertyChangeLogError, PostgresRepositoryError};
use fleetiq_history::{SemanticLease, SemanticUnresolved, SemanticWorkRepository};
use fleetiq_semantics::AssetPropertyChange;
use sqlx::PgPool;

/// Fenced, database-clock semantic work over immutable decoded observations.
#[derive(Clone, Debug)]
pub struct PostgresSemanticWorkRepository {
    pool: PgPool,
}
impl PostgresSemanticWorkRepository {
    /// Uses a fully migrated pool. Does not spawn workers or mutate schema.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
impl SemanticWorkRepository for PostgresSemanticWorkRepository {
    type Error = PostgresSemanticWorkError;
    async fn seed(&self) -> Result<(), Self::Error> {
        self.seed_batch().await
    }
    async fn claim(&self) -> Result<Option<SemanticLease>, Self::Error> {
        self.claim_one().await
    }
    async fn pin(
        &self,
        lease: &SemanticLease,
        change: &AssetPropertyChange,
    ) -> Result<bool, Self::Error> {
        self.pin_selection(lease, change).await
    }
    async fn complete(
        &self,
        lease: &SemanticLease,
        change: &AssetPropertyChange,
    ) -> Result<bool, Self::Error> {
        self.finish(lease, change).await
    }
    async fn retain(
        &self,
        lease: &SemanticLease,
        reason: SemanticUnresolved,
    ) -> Result<bool, Self::Error> {
        self.retain_reason(lease, reason).await
    }
}
/// Storage causes remain inspectable; routine formatting exposes no source data.
pub enum PostgresSemanticWorkError {
    /// Work storage or row reconstruction failed.
    Repository(PostgresRepositoryError),
    /// Immutable lineage validation or history persistence failed.
    History(PostgresAssetPropertyChangeLogError),
    /// The supplied lease/change or stored selection disagreed.
    Invalid,
}
impl From<PostgresRepositoryError> for PostgresSemanticWorkError {
    fn from(error: PostgresRepositoryError) -> Self {
        Self::Repository(error)
    }
}
impl From<PostgresAssetPropertyChangeLogError> for PostgresSemanticWorkError {
    fn from(error: PostgresAssetPropertyChangeLogError) -> Self {
        Self::History(error)
    }
}
impl std::fmt::Display for PostgresSemanticWorkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("durable semantic work failed")
    }
}
impl std::fmt::Debug for PostgresSemanticWorkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
impl std::error::Error for PostgresSemanticWorkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Repository(e) => Some(e),
            Self::History(e) => Some(e),
            Self::Invalid => None,
        }
    }
}
