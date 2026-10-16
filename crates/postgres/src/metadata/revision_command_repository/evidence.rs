//! Exact immutable evidence lookup never infers attribution for legacy facts.
use super::{
    ActorId, MetadataRevision, PostgresRelationshipRepository, PostgresRepositoryError,
    PostgresRevisionCommandRepository, PostgresSignalBindingRepository, RevisionOutcome,
    RevisionReceipt, classify_database_error,
};
use crate::error::corrupt;
use bigdecimal::BigDecimal;
use fleetiq_metadata::{RelationshipRepository, SignalBindingRepository};

pub(super) async fn insert(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    value: &MetadataRevision,
    actor: &ActorId,
) -> Result<(), PostgresRepositoryError> {
    let number = BigDecimal::from(value.revision().get());
    match value {
        MetadataRevision::Relationship(_) => {
            sqlx::query!(
                "INSERT INTO metadata.relationship_revision_records (tenant_id,relationship_id,revision,actor_id) VALUES ($1,$2,$3,$4)",
                value.tenant_id().as_str(),
                value.id(),
                number,
                actor.as_str()
            )
            .execute(&mut **transaction)
            .await
        }
        MetadataRevision::Binding(_) => {
            sqlx::query!(
                "INSERT INTO metadata.signal_binding_revision_records (tenant_id,binding_id,revision,actor_id) VALUES ($1,$2,$3,$4)",
                value.tenant_id().as_str(),
                value.id(),
                number,
                actor.as_str()
            )
            .execute(&mut **transaction)
            .await
        }
    }
    .map_err(classify_database_error)?;
    Ok(())
}
impl PostgresRevisionCommandRepository {
    /// Reads exact immutable content, not today's head; later corrections cannot
    /// invalidate a retry of an earlier attributed revision.
    pub(super) async fn resolve(
        &self,
        requested: &MetadataRevision,
    ) -> Result<RevisionOutcome, PostgresRepositoryError> {
        let tenant = requested.tenant_id();
        let number = BigDecimal::from(requested.revision().get());
        let stored = match requested {
            MetadataRevision::Relationship(value) => {
                PostgresRelationshipRepository::new(self.pool.clone())
                    .find_revision(tenant, value.id(), value.revision())
                    .await?
                    .map(MetadataRevision::Relationship)
            }
            MetadataRevision::Binding(value) => {
                PostgresSignalBindingRepository::new(self.pool.clone())
                    .find_revision(tenant, value.id(), value.revision())
                    .await?
                    .map(MetadataRevision::Binding)
            }
        };
        let Some(stored) = stored.filter(|v| requested.same_content(v)) else {
            return Ok(RevisionOutcome::Conflict);
        };
        let actor = match requested {
            MetadataRevision::Relationship(_) => {
                sqlx::query_scalar!(
                    "SELECT actor_id FROM metadata.relationship_revision_records WHERE tenant_id=$1 AND relationship_id=$2 AND revision=$3",
                    tenant.as_str(),
                    requested.id(),
                    number
                )
                .fetch_optional(&self.pool)
                .await
            }
            MetadataRevision::Binding(_) => {
                sqlx::query_scalar!(
                    "SELECT actor_id FROM metadata.signal_binding_revision_records WHERE tenant_id=$1 AND binding_id=$2 AND revision=$3",
                    tenant.as_str(),
                    requested.id(),
                    number
                )
                .fetch_optional(&self.pool)
                .await
            }
        }
        .map_err(classify_database_error)?;
        let Some(actor) = actor else {
            return Ok(RevisionOutcome::Conflict);
        };
        Ok(RevisionOutcome::AlreadyAppended(RevisionReceipt::new(
            ActorId::new(actor).map_err(|e| corrupt("revision_records.actor_id", e))?,
            stored.recorded_at(),
        )))
    }
}
