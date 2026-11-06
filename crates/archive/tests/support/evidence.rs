//! Exact evidence and hostile segment regression fixtures.
use fleetiq_domain::{
    AssetId, DefinitionVersion, DeviceEndpointId, PropertyDefinitionId, PropertyDefinitionRef,
    PropertyValue, RecordRevision, SignalBindingId, SignalSource, SignalTarget, SourceSignalId,
    TenantId, UnitId, UnixTimeMillis,
};
use fleetiq_history::StoredPropertyChange;
use fleetiq_semantics::{AssetPropertyChangeId, EventTimeBasis, ResolutionContext};
use fleetiq_telemetry::{
    AcceptedMessageId, DecodedMeasurement, DecodedObservation, DecodedObservationId,
    ObservationQuality, ObservationTimes, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};
pub fn evidence(index: u32, value: PropertyValue) -> StoredPropertyChange {
    let unit = value.is_numeric().then(|| UnitId::new("volt").unwrap());
    let quality = match index % 3 {
        0 => ObservationQuality::Good,
        1 => ObservationQuality::Uncertain,
        _ => ObservationQuality::Invalid,
    };
    let device = (index.is_multiple_of(2)).then_some(UnixTimeMillis::new(i64::MIN));
    let basis = if device.is_some() {
        EventTimeBasis::DeviceRecorded
    } else {
        EventTimeBasis::ReceivedAt
    };
    let event = device.unwrap_or(UnixTimeMillis::new(i64::MAX));
    let source = DecodedObservation::new(
        DecodedObservationId::new(
            AcceptedMessageId::new(format!("message-{index}")).unwrap(),
            ProtocolPackageRef::new(
                ProtocolPackageId::new("vendor.binary").unwrap(),
                ProtocolPackageVersion::new("2026.11").unwrap(),
            ),
            index,
        ),
        TenantId::new("tenant-a").unwrap(),
        SignalSource::new(
            AssetId::new("device").unwrap(),
            DeviceEndpointId::new("can").unwrap(),
            SourceSignalId::new("value").unwrap(),
        ),
        DecodedMeasurement::new(value, unit, quality).unwrap(),
        ObservationTimes::new(device, UnixTimeMillis::new(i64::MAX)),
    );
    StoredPropertyChange::new(
        AssetPropertyChangeId::from_parts(
            source.id().clone(),
            SignalBindingId::new("binding").unwrap(),
            RecordRevision::new(u64::MAX).unwrap(),
            basis,
        ),
        source,
        SignalTarget::new(
            AssetId::new("target").unwrap(),
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new("value").unwrap(),
                DefinitionVersion::new(u32::MAX).unwrap(),
            ),
        ),
        event,
        ResolutionContext::new(UnixTimeMillis::new(i64::MAX), UnixTimeMillis::new(-1)),
    )
    .unwrap()
}
