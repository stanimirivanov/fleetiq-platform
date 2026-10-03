//! Exact tenant-scoped read of one immutable semantic interpretation.

use std::future::Future;

use bigdecimal::BigDecimal;
use fleetiq_domain::{
    AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, SignalTarget,
    TenantId, UnixTimeMillis,
};
use fleetiq_history::{AssetPropertyChangeView, DecodedObservationView, StoredPropertyChange};
use fleetiq_semantics::{AssetPropertyChangeId, ResolutionContext};
use sqlx::PgPool;

use super::{
    decoded_observation_view::PostgresDecodedObservationView, property_change_log::basis_name,
};
use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};

/// PostgreSQL view of one exact immutable property interpretation.
#[derive(Clone, Debug)]
pub struct PostgresAssetPropertyChangeView {
    pool: PgPool,
    observations: PostgresDecodedObservationView,
}

impl PostgresAssetPropertyChangeView {
    /// Uses a pool connected to the fully migrated `FleetIQ` database.
    pub fn new(pool: PgPool) -> Self {
        Self {
            observations: PostgresDecodedObservationView::new(pool.clone()),
            pool,
        }
    }

    /// Reads the stored interpretation and its immutable source without
    /// re-resolving the binding under today's metadata snapshot.
    ///
    /// The first statement selects the exact history key and binding target.
    /// The second reuses the validated decoded-observation mapper. The history,
    /// binding, and source facts are immutable, so the second read cannot pick
    /// up a changed source fact.
    /// are immutable, so the second read cannot pick up a changed source fact.
    async fn lookup(
        &self,
        tenant_id: &TenantId,
        id: &AssetPropertyChangeId,
    ) -> Result<Option<StoredPropertyChange>, PostgresRepositoryError> {
        let observation_id = id.observation_id();
        let package = observation_id.decoded_by();
        let row = sqlx::query_as!(
            StoredChangeRow,
            r"
            SELECT change.event_at_ms, change.resolution_effective_at_ms,
                   change.resolution_known_at_ms, binding.target_asset_id,
                   binding.target_property_id, binding.target_property_version
            FROM history.asset_property_changes AS change
            JOIN metadata.signal_binding_revisions AS binding
              ON binding.tenant_id = change.tenant_id
             AND binding.binding_id = change.binding_id
             AND binding.revision = change.binding_revision
            WHERE change.tenant_id = $1 AND change.message_id = $2
              AND change.protocol_package_id = $3
              AND change.protocol_package_version = $4
              AND change.output_index = $5 AND change.binding_id = $6
              AND change.binding_revision = $7 AND change.event_time_basis = $8
            ",
            tenant_id.as_str(),
            observation_id.message_id().as_str(),
            package.id().as_str(),
            package.version().as_str(),
            i64::from(observation_id.output_index()),
            id.binding_id().as_str(),
            BigDecimal::from(id.binding_revision().get()),
            basis_name(id.event_time_basis()),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(classify_database_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let observation = self
            .observations
            .find(tenant_id, observation_id)
            .await?
            .ok_or_else(|| {
                corrupt(
                    "history.asset_property_changes",
                    "source observation missing",
                )
            })?;
        row.into_domain(id, observation).map(Some)
    }
}

impl AssetPropertyChangeView for PostgresAssetPropertyChangeView {
    type Error = PostgresRepositoryError;

    fn find<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        id: &'a AssetPropertyChangeId,
    ) -> impl Future<Output = Result<Option<StoredPropertyChange>, Self::Error>> + Send + 'a {
        self.lookup(tenant_id, id)
    }
}

/// History-owned fields and the exact immutable binding target.
struct StoredChangeRow {
    event_at_ms: i64,
    resolution_effective_at_ms: i64,
    resolution_known_at_ms: i64,
    target_asset_id: String,
    target_property_id: String,
    target_property_version: i64,
}

impl StoredChangeRow {
    fn into_domain(
        self,
        id: &AssetPropertyChangeId,
        observation: fleetiq_telemetry::DecodedObservation,
    ) -> Result<StoredPropertyChange, PostgresRepositoryError> {
        let property_version = u32::try_from(self.target_property_version)
            .map_err(|error| corrupt("signal_binding_revisions.target_property_version", error))?;
        let target = SignalTarget::new(
            AssetId::new(self.target_asset_id)
                .map_err(|error| corrupt("signal_binding_revisions.target_asset_id", error))?,
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new(self.target_property_id).map_err(|error| {
                    corrupt("signal_binding_revisions.target_property_id", error)
                })?,
                DefinitionVersion::new(property_version).map_err(|error| {
                    corrupt("signal_binding_revisions.target_property_version", error)
                })?,
            ),
        );
        StoredPropertyChange::new(
            id.clone(),
            observation,
            target,
            UnixTimeMillis::new(self.event_at_ms),
            ResolutionContext::new(
                UnixTimeMillis::new(self.resolution_effective_at_ms),
                UnixTimeMillis::new(self.resolution_known_at_ms),
            ),
        )
        .map_err(|error| corrupt("history.asset_property_changes", error))
    }
}
