//! Transaction composition reuses the strict append algorithms and their locks.
mod evidence;
use super::{PostgresRelationshipRepository, PostgresSignalBindingRepository};
use crate::PostgresRepositoryError;
use crate::error::classify_database_error;
use fleetiq_domain::ActorId;
use fleetiq_metadata::{
    MetadataRevision, RevisionCommandRepository, RevisionOutcome, RevisionReceipt,
};
use sqlx::PgPool;

/// Attributed revision adapter; strict repository callers keep their old contract.
#[derive(Clone, Debug)]
pub struct PostgresRevisionCommandRepository {
    pool: PgPool,
}
impl PostgresRevisionCommandRepository {
    /// Shares the configured pool without changing isolation or retry policy.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
impl RevisionCommandRepository for PostgresRevisionCommandRepository {
    type Error = PostgresRepositoryError;
    /// Parent head, complete fact and actor evidence share a single transaction.
    ///
    /// The existing algorithm serializes by stable identity. A losing/gapped
    /// append is rolled back before exact winner lookup in a fresh snapshot.
    /// Immutable facts and attribution allow safe separate reads on retries.
    async fn append(
        &self,
        requested: &MetadataRevision,
        actor: &ActorId,
    ) -> Result<RevisionOutcome, Self::Error> {
        let mut transaction = self.pool.begin().await.map_err(classify_database_error)?;
        let result = match requested {
            MetadataRevision::Relationship(v) => {
                PostgresRelationshipRepository::append_revision_on(&mut transaction, v).await
            }
            MetadataRevision::Binding(v) => {
                PostgresSignalBindingRepository::append_revision_on(&mut transaction, v).await
            }
        };
        if let Err(error) = result {
            transaction
                .rollback()
                .await
                .map_err(classify_database_error)?;
            return match error {
                PostgresRepositoryError::UnexpectedRevision { .. }
                | PostgresRepositoryError::RevisionExhausted => self.resolve(requested).await,
                PostgresRepositoryError::RecordedTimeRegression { .. } => {
                    Ok(RevisionOutcome::Conflict)
                }
                _ => Err(error),
            };
        }
        evidence::insert(&mut transaction, requested, actor).await?;
        transaction
            .commit()
            .await
            .map_err(classify_database_error)?;
        Ok(RevisionOutcome::Appended(RevisionReceipt::new(
            actor.clone(),
            requested.recorded_at(),
        )))
    }
}
