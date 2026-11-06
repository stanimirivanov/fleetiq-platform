//! Exact HTTP evidence with a statement deadline inside the admission lease.
use super::{PostgresAssetPropertyChangeView, row::HistoryRow};
use crate::error::{PostgresRepositoryError, classify_database_error};
use bigdecimal::BigDecimal;
use fleetiq_access::{AuthorizedTenant, ReadTelemetry};
use fleetiq_domain::SignalTarget;
use fleetiq_history::StoredPropertyChange;
use fleetiq_semantics::AssetPropertyChangeId;
impl PostgresAssetPropertyChangeView {
    /// Reads exact original evidence in one bounded query under current telemetry authority.
    /// # Errors
    /// Returns statement deadline, storage and row-reconstruction failures.
    pub async fn find_property_bounded(
        &self,
        grant: AuthorizedTenant<ReadTelemetry>,
        target: &SignalTarget,
        id: &AssetPropertyChangeId,
    ) -> Result<Option<StoredPropertyChange>, PostgresRepositoryError> {
        let observation = id.observation_id();
        let package = observation.decoded_by();
        let mut tx = self.pool.begin().await.map_err(classify_database_error)?;
        sqlx::query("SET LOCAL statement_timeout = '5s'")
            .execute(&mut *tx)
            .await
            .map_err(classify_database_error)?;
        let row = sqlx::query_file_as!(
            HistoryRow,
            "src/history/property_change_view/bounded_exact.sql",
            grant.tenant_id().as_str(),
            target.asset_id().as_str(),
            target.property().id().as_str(),
            i64::from(target.property().version().get()),
            observation.message_id().as_str(),
            package.id().as_str(),
            package.version().as_str(),
            i64::from(observation.output_index()),
            id.binding_id().as_str(),
            BigDecimal::from(id.binding_revision().get()),
            super::basis_name(id.event_time_basis()),
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(classify_database_error)?;
        tx.commit().await.map_err(classify_database_error)?;
        row.map(|r| r.into_domain(grant.tenant_id())).transpose()
    }
}
