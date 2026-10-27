//! Fenced interpretation writes. Lock order: tenant, work, immutable history.
use super::super::property_change_log::{PostgresAssetPropertyChangeLog, basis_name};
use super::{PostgresSemanticWorkError, PostgresSemanticWorkRepository};
use crate::error::classify_database_error;
use bigdecimal::BigDecimal;
use fleetiq_history::{SemanticLease, SemanticUnresolved};
use fleetiq_semantics::AssetPropertyChange;

impl PostgresSemanticWorkRepository {
    /// A share lock orders completion with lifecycle suspension; an in-flight
    /// authorized transaction completes before suspension, never after its commit.
    async fn lock_live(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        lease: &SemanticLease,
    ) -> Result<bool, PostgresSemanticWorkError> {
        let active = sqlx::query_scalar!(
            "SELECT is_active FROM metadata.tenants WHERE tenant_id=$1 FOR SHARE",
            lease.request.tenant_id().as_str()
        )
        .fetch_optional(&mut **tx)
        .await
        .map_err(classify_database_error)?;
        if active != Some(true) {
            return Ok(false);
        }
        let id = lease.request.id();
        let valid = sqlx::query_scalar!(
            r#"
            SELECT TRUE AS "valid!" FROM history.semantic_work
            WHERE tenant_id=$1 AND message_id=$2 AND protocol_package_id=$3
            AND protocol_package_version=$4 AND output_index=$5
            AND status='pending' AND generation=$6 AND known_at_ms=$7 AND event_time_basis=$8
            AND lease_until_ms>floor(extract(epoch FROM clock_timestamp())*1000)::bigint
            FOR UPDATE
        "#,
            lease.request.tenant_id().as_str(),
            id.message_id().as_str(),
            id.decoded_by().id().as_str(),
            id.decoded_by().version().as_str(),
            i64::from(id.output_index()),
            lease.generation,
            lease.request.known_at().get(),
            basis_name(lease.request.basis())
        )
        .fetch_optional(&mut **tx)
        .await
        .map_err(classify_database_error)?;
        Ok(valid == Some(true))
    }
    pub(super) async fn pin_selection(
        &self,
        lease: &SemanticLease,
        change: &AssetPropertyChange,
    ) -> Result<bool, PostgresSemanticWorkError> {
        validate_change(lease, change)?;
        let mut tx = self.pool.begin().await.map_err(classify_database_error)?;
        if !self.lock_live(&mut tx, lease).await? {
            return Ok(false);
        }
        if !PostgresAssetPropertyChangeLog::matches_durable_facts(change, &mut tx).await? {
            return Err(PostgresSemanticWorkError::Invalid);
        }
        let id = lease.request.id();
        let changed = sqlx::query!(
            r#"
            UPDATE history.semantic_work SET binding_id=$6,binding_revision=$7
            WHERE tenant_id=$1 AND message_id=$2 AND protocol_package_id=$3
              AND protocol_package_version=$4 AND output_index=$5
              AND (binding_id IS NULL OR (binding_id=$6 AND binding_revision=$7))
        "#,
            lease.request.tenant_id().as_str(),
            id.message_id().as_str(),
            id.decoded_by().id().as_str(),
            id.decoded_by().version().as_str(),
            i64::from(id.output_index()),
            change.id().binding_id().as_str(),
            BigDecimal::from(change.id().binding_revision().get())
        )
        .execute(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        tx.commit().await.map_err(classify_database_error)?;
        Ok(changed.rows_affected() == 1)
    }
    pub(super) async fn finish(
        &self,
        lease: &SemanticLease,
        change: &AssetPropertyChange,
    ) -> Result<bool, PostgresSemanticWorkError> {
        validate_change(lease, change)?;
        let mut tx = self.pool.begin().await.map_err(classify_database_error)?;
        if !self.lock_live(&mut tx, lease).await? {
            return Ok(false);
        }
        let id = lease.request.id();
        // Update first in this transaction, verifying the durable selection; an
        // append failure rolls the terminal marker back too. No pool escape.
        let changed = sqlx::query!(
            r#"
            UPDATE history.semantic_work SET status='completed'
            WHERE tenant_id=$1 AND message_id=$2 AND protocol_package_id=$3
              AND protocol_package_version=$4 AND output_index=$5
              AND binding_id=$6 AND binding_revision=$7
        "#,
            lease.request.tenant_id().as_str(),
            id.message_id().as_str(),
            id.decoded_by().id().as_str(),
            id.decoded_by().version().as_str(),
            i64::from(id.output_index()),
            change.id().binding_id().as_str(),
            BigDecimal::from(change.id().binding_revision().get())
        )
        .execute(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        if changed.rows_affected() != 1 {
            return Ok(false);
        }
        PostgresAssetPropertyChangeLog::append_in_transaction(change, &mut tx).await?;
        tx.commit().await.map_err(classify_database_error)?;
        Ok(true)
    }
    pub(super) async fn retain_reason(
        &self,
        lease: &SemanticLease,
        reason: SemanticUnresolved,
    ) -> Result<bool, PostgresSemanticWorkError> {
        let mut tx = self.pool.begin().await.map_err(classify_database_error)?;
        if !self.lock_live(&mut tx, lease).await? {
            return Ok(false);
        }
        let id = lease.request.id();
        let reason = match reason {
            SemanticUnresolved::NoBinding => "no_binding",
            SemanticUnresolved::Ambiguous => "ambiguous",
            SemanticUnresolved::MissingDefinition => "missing_definition",
            SemanticUnresolved::ValueKind => "value_kind",
            SemanticUnresolved::Unit => "unit",
        };
        let changed = sqlx::query!(
            r#"
            UPDATE history.semantic_work SET status='unresolved',reason=$6
            WHERE tenant_id=$1 AND message_id=$2 AND protocol_package_id=$3
              AND protocol_package_version=$4 AND output_index=$5 AND binding_id IS NULL
        "#,
            lease.request.tenant_id().as_str(),
            id.message_id().as_str(),
            id.decoded_by().id().as_str(),
            id.decoded_by().version().as_str(),
            i64::from(id.output_index()),
            reason
        )
        .execute(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        tx.commit().await.map_err(classify_database_error)?;
        Ok(changed.rows_affected() == 1)
    }
}
fn validate_change(
    lease: &SemanticLease,
    change: &AssetPropertyChange,
) -> Result<(), PostgresSemanticWorkError> {
    if change.tenant_id() != lease.request.tenant_id()
        || change.id().observation_id() != lease.request.id()
        || change.id().event_time_basis() != lease.request.basis()
        || change.bound_observation().context().known_at() != lease.request.known_at()
    {
        return Err(PostgresSemanticWorkError::Invalid);
    }
    Ok(())
}
