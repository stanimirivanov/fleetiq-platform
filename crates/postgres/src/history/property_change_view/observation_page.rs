//! Keyset page of interpretations for one immutable decoded output.

use bigdecimal::BigDecimal;
use fleetiq_domain::{RecordRevision, SignalBindingId, TenantId};
use fleetiq_history::{
    DecodedObservationView, ObservationInterpretationPage, ObservationInterpretationPageQuery,
    StoredPropertyChange,
};
use fleetiq_semantics::{AssetPropertyChangeId, EventTimeBasis};
use fleetiq_telemetry::DecodedObservation;

use super::{PostgresAssetPropertyChangeView, StoredChangeRow, basis_name};
use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};

impl PostgresAssetPropertyChangeView {
    /// Reads at most limit plus one history rows, then loads the shared source
    /// observation once. The primary-key prefix supports exact source selection
    /// and the remaining key columns provide deterministic keyset order.
    pub(super) async fn list_observation_page(
        &self,
        tenant_id: &TenantId,
        query: &ObservationInterpretationPageQuery,
    ) -> Result<ObservationInterpretationPage, PostgresRepositoryError> {
        let source_id = query.observation_id();
        let package = source_id.decoded_by();
        let cursor = query.after();
        let cursor_binding = cursor.map(|id| id.binding_id().as_str());
        let cursor_revision = cursor.map(|id| BigDecimal::from(id.binding_revision().get()));
        let cursor_basis = cursor.map(|id| basis_name(id.event_time_basis()));
        let mut rows = sqlx::query_as!(
            PagedChangeRow,
            r"
            SELECT change.binding_id, change.binding_revision,
                   change.event_time_basis, change.event_at_ms,
                   change.resolution_effective_at_ms,
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
              AND change.output_index = $5
              AND (
                  $6::text IS NULL
                  OR (change.binding_id, change.binding_revision, change.event_time_basis)
                     > ($6::text, $7::numeric, $8::text)
              )
            ORDER BY change.binding_id, change.binding_revision,
                     change.event_time_basis
            LIMIT $9
            ",
            tenant_id.as_str(),
            source_id.message_id().as_str(),
            package.id().as_str(),
            package.version().as_str(),
            i64::from(source_id.output_index()),
            cursor_binding,
            cursor_revision,
            cursor_basis,
            i64::from(query.limit()) + 1,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(classify_database_error)?;
        if rows.is_empty() {
            return Ok(ObservationInterpretationPage::new(Vec::new(), None));
        }
        let has_next = rows.len() > usize::from(query.limit());
        rows.truncate(usize::from(query.limit()));
        let observation = self
            .observations
            .find(tenant_id, source_id)
            .await?
            .ok_or_else(|| {
                corrupt(
                    "history.asset_property_changes",
                    "source observation missing",
                )
            })?;
        let changes = rows
            .into_iter()
            .map(|row| row.into_domain(&observation))
            .collect::<Result<Vec<StoredPropertyChange>, _>>()?;
        let next_after = if has_next {
            changes.last().map(|change| change.id().clone())
        } else {
            None
        };
        Ok(ObservationInterpretationPage::new(changes, next_after))
    }
}

/// One joined history/binding row for the selected decoded source.
struct PagedChangeRow {
    binding_id: String,
    binding_revision: BigDecimal,
    event_time_basis: String,
    event_at_ms: i64,
    resolution_effective_at_ms: i64,
    resolution_known_at_ms: i64,
    target_asset_id: String,
    target_property_id: String,
    target_property_version: i64,
}

impl PagedChangeRow {
    fn into_domain(
        self,
        observation: &DecodedObservation,
    ) -> Result<StoredPropertyChange, PostgresRepositoryError> {
        let binding_id = SignalBindingId::new(self.binding_id)
            .map_err(|error| corrupt("asset_property_changes.binding_id", error))?;
        let revision = self
            .binding_revision
            .to_string()
            .parse::<u64>()
            .map_err(|error| corrupt("asset_property_changes.binding_revision", error))?;
        let revision = RecordRevision::new(revision)
            .map_err(|error| corrupt("asset_property_changes.binding_revision", error))?;
        let basis = match self.event_time_basis.as_str() {
            "device_recorded" => EventTimeBasis::DeviceRecorded,
            "received_at" => EventTimeBasis::ReceivedAt,
            _ => {
                return Err(corrupt(
                    "asset_property_changes.event_time_basis",
                    "unknown event-time basis",
                ));
            }
        };
        let id = AssetPropertyChangeId::from_parts(
            observation.id().clone(),
            binding_id,
            revision,
            basis,
        );
        StoredChangeRow {
            event_at_ms: self.event_at_ms,
            resolution_effective_at_ms: self.resolution_effective_at_ms,
            resolution_known_at_ms: self.resolution_known_at_ms,
            target_asset_id: self.target_asset_id,
            target_property_id: self.target_property_id,
            target_property_version: self.target_property_version,
        }
        .into_domain(&id, observation.clone())
    }
}
