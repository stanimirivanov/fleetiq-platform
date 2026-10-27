use bigdecimal::BigDecimal;
use fleetiq_domain::{
    AssetId, DefinitionVersion, DeviceEndpointId, EffectiveInterval, PropertyDefinitionId,
    PropertyDefinitionRef, RecordedRevision, SignalBinding, SignalBindingId, SignalSource,
    SignalTarget, SourceSignalId, TenantId, UnixTimeMillis,
};

use crate::error::{PostgresRepositoryError, corrupt};

use super::common::parse_revision;

#[derive(Debug)]
pub(in crate::metadata) struct SignalBindingRevisionRow {
    pub(in crate::metadata) binding_id: String,
    pub(in crate::metadata) revision: BigDecimal,
    pub(in crate::metadata) recorded_at_ms: i64,
    pub(in crate::metadata) source_device_asset_id: String,
    pub(in crate::metadata) source_endpoint_id: String,
    pub(in crate::metadata) source_signal_id: String,
    pub(in crate::metadata) target_asset_id: String,
    pub(in crate::metadata) target_property_id: String,
    pub(in crate::metadata) target_property_version: i64,
    pub(in crate::metadata) effective_start_ms: i64,
    pub(in crate::metadata) effective_end_ms: Option<i64>,
}

impl SignalBindingRevisionRow {
    pub(in crate::metadata) fn into_domain(
        self,
        tenant_id: TenantId,
    ) -> Result<SignalBinding, PostgresRepositoryError> {
        let interval = EffectiveInterval::new(
            UnixTimeMillis::new(self.effective_start_ms),
            self.effective_end_ms.map(UnixTimeMillis::new),
        )
        .map_err(|error| corrupt("signal_binding_revisions.effective_interval", error))?;

        Ok(SignalBinding::new(
            tenant_id,
            SignalBindingId::new(self.binding_id)
                .map_err(|error| corrupt("signal_binding_revisions.binding_id", error))?,
            RecordedRevision::new(
                parse_revision("signal_binding_revisions.revision", &self.revision)?,
                UnixTimeMillis::new(self.recorded_at_ms),
            ),
            SignalSource::new(
                AssetId::new(self.source_device_asset_id).map_err(|error| {
                    corrupt("signal_binding_revisions.source_device_asset_id", error)
                })?,
                DeviceEndpointId::new(self.source_endpoint_id).map_err(|error| {
                    corrupt("signal_binding_revisions.source_endpoint_id", error)
                })?,
                SourceSignalId::new(self.source_signal_id)
                    .map_err(|error| corrupt("signal_binding_revisions.source_signal_id", error))?,
            ),
            SignalTarget::new(
                AssetId::new(self.target_asset_id)
                    .map_err(|error| corrupt("signal_binding_revisions.target_asset_id", error))?,
                PropertyDefinitionRef::new(
                    PropertyDefinitionId::new(self.target_property_id).map_err(|error| {
                        corrupt("signal_binding_revisions.target_property_id", error)
                    })?,
                    definition_version(
                        "signal_binding_revisions.target_property_version",
                        self.target_property_version,
                    )?,
                ),
            ),
            interval,
        ))
    }
}

fn definition_version(
    field: &'static str,
    value: i64,
) -> Result<DefinitionVersion, PostgresRepositoryError> {
    let value = u32::try_from(value).map_err(|error| corrupt(field, error))?;
    DefinitionVersion::new(value).map_err(|error| corrupt(field, error))
}
