//! Shared checked immutable interpretation/source row; no per-result lookups.
use super::StoredChangeRow;
use crate::{
    error::{PostgresRepositoryError, corrupt},
    history::decoded_observation_view::StoredObservationRow,
};
use bigdecimal::BigDecimal;
use fleetiq_domain::{RecordRevision, SignalBindingId, TenantId};
use fleetiq_semantics::{AssetPropertyChangeId, EventTimeBasis};
use fleetiq_telemetry::{
    AcceptedMessageId, DecodedObservationId, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};
/// Flat checked SQL row: no JSON aggregation or per-result source lookup.
pub(crate) struct HistoryRow {
    pub(crate) message_id: String,
    pub(crate) protocol_package_id: String,
    pub(crate) protocol_package_version: String,
    pub(crate) output_index: i64,
    pub(crate) binding_id: String,
    pub(crate) binding_revision: BigDecimal,
    pub(crate) event_time_basis: String,
    pub(crate) event_at_ms: i64,
    pub(crate) resolution_effective_at_ms: i64,
    pub(crate) resolution_known_at_ms: i64,
    pub(crate) target_asset_id: String,
    pub(crate) target_property_id: String,
    pub(crate) target_property_version: i64,
    pub(crate) device_asset_id: String,
    pub(crate) received_at_ms: i64,
    pub(crate) endpoint_id: String,
    pub(crate) signal_id: String,
    pub(crate) device_recorded_at_ms: Option<i64>,
    pub(crate) value_kind: String,
    pub(crate) value_boolean: Option<bool>,
    pub(crate) value_integer: Option<i64>,
    pub(crate) value_decimal: Option<BigDecimal>,
    pub(crate) value_text: Option<String>,
    pub(crate) source_unit: Option<String>,
    pub(crate) quality: String,
}
impl HistoryRow {
    pub(crate) fn into_domain(
        self,
        tenant: &TenantId,
    ) -> Result<fleetiq_history::StoredPropertyChange, PostgresRepositoryError> {
        let source_id = DecodedObservationId::new(
            AcceptedMessageId::new(self.message_id)
                .map_err(|e| corrupt("history.property_page.identity", e))?,
            ProtocolPackageRef::new(
                ProtocolPackageId::new(self.protocol_package_id)
                    .map_err(|e| corrupt("history.property_page.identity", e))?,
                ProtocolPackageVersion::new(self.protocol_package_version)
                    .map_err(|e| corrupt("history.property_page.identity", e))?,
            ),
            u32::try_from(self.output_index)
                .map_err(|e| corrupt("history.property_page.output_index", e))?,
        );
        let basis = match self.event_time_basis.as_str() {
            "device_recorded" => EventTimeBasis::DeviceRecorded,
            "received_at" => EventTimeBasis::ReceivedAt,
            _ => {
                return Err(corrupt(
                    "history.property_page.event_time_basis",
                    "unknown time basis",
                ));
            }
        };
        let revision = self
            .binding_revision
            .to_string()
            .parse::<u64>()
            .map_err(|e| corrupt("history.property_page.binding_revision", e))?;
        let id = AssetPropertyChangeId::from_parts(
            source_id.clone(),
            SignalBindingId::new(self.binding_id)
                .map_err(|e| corrupt("history.property_page.identity", e))?,
            RecordRevision::new(revision)
                .map_err(|e| corrupt("history.property_page.binding_revision", e))?,
            basis,
        );
        let observation = StoredObservationRow {
            device_asset_id: self.device_asset_id,
            received_at_ms: self.received_at_ms,
            endpoint_id: self.endpoint_id,
            signal_id: self.signal_id,
            device_recorded_at_ms: self.device_recorded_at_ms,
            value_kind: self.value_kind,
            value_boolean: self.value_boolean,
            value_integer: self.value_integer,
            value_decimal: self.value_decimal,
            value_text: self.value_text,
            source_unit: self.source_unit,
            quality: self.quality,
        }
        .into_domain(tenant, &source_id)?;
        StoredChangeRow {
            event_at_ms: self.event_at_ms,
            resolution_effective_at_ms: self.resolution_effective_at_ms,
            resolution_known_at_ms: self.resolution_known_at_ms,
            target_asset_id: self.target_asset_id,
            target_property_id: self.target_property_id,
            target_property_version: self.target_property_version,
        }
        .into_domain(&id, observation)
    }
}
