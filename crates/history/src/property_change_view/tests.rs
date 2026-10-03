use fleetiq_domain::{
    AssetId, DefinitionVersion, DeviceEndpointId, PropertyDefinitionId, PropertyDefinitionRef,
    PropertyValue, RecordRevision, SignalBindingId, SignalSource, SourceSignalId, UnitId,
};
use fleetiq_telemetry::{
    AcceptedMessageId, DecodedMeasurement, DecodedObservationId, ObservationQuality,
    ObservationTimes, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};

use super::*;

fn observation(message_id: &str) -> DecodedObservation {
    DecodedObservation::new(
        DecodedObservationId::new(
            AcceptedMessageId::new(message_id).unwrap(),
            ProtocolPackageRef::new(
                ProtocolPackageId::new("vendor.protocol").unwrap(),
                ProtocolPackageVersion::new("1").unwrap(),
            ),
            0,
        ),
        TenantId::new("tenant-a").unwrap(),
        SignalSource::new(
            AssetId::new("gateway-1").unwrap(),
            DeviceEndpointId::new("can-primary").unwrap(),
            SourceSignalId::new("bus.voltage").unwrap(),
        ),
        DecodedMeasurement::new(
            PropertyValue::Integer(48),
            Some(UnitId::new("volt").unwrap()),
            ObservationQuality::Good,
        )
        .unwrap(),
        ObservationTimes::new(Some(UnixTimeMillis::new(1000)), UnixTimeMillis::new(1100)),
    )
}

fn identity(observation: &DecodedObservation) -> AssetPropertyChangeId {
    AssetPropertyChangeId::from_parts(
        observation.id().clone(),
        SignalBindingId::new("binding-1").unwrap(),
        RecordRevision::new(1).unwrap(),
        EventTimeBasis::DeviceRecorded,
    )
}

fn target() -> SignalTarget {
    SignalTarget::new(
        AssetId::new("power-system-1").unwrap(),
        PropertyDefinitionRef::new(
            PropertyDefinitionId::new("electrical.voltage").unwrap(),
            DefinitionVersion::new(1).unwrap(),
        ),
    )
}

#[test]
fn preserves_exact_interpretation_and_first_resolution_context() {
    let source = observation("message-1");
    let id = identity(&source);
    let first = ResolutionContext::new(UnixTimeMillis::new(1000), UnixTimeMillis::new(150));
    let stored = StoredPropertyChange::new(
        id.clone(),
        source.clone(),
        target(),
        UnixTimeMillis::new(1000),
        first,
    )
    .unwrap();
    assert_eq!(stored.id(), &id);
    assert_eq!(stored.observation(), &source);
    assert_eq!(stored.tenant_id().as_str(), "tenant-a");
    assert_eq!(stored.target().asset_id().as_str(), "power-system-1");
    assert_eq!(stored.first_resolution(), first);
}

#[test]
fn rejects_mismatched_source_identity_and_event_time() {
    let source = observation("message-1");
    let id = identity(&source);
    let context = ResolutionContext::new(UnixTimeMillis::new(1000), UnixTimeMillis::new(150));
    assert_eq!(
        StoredPropertyChange::new(
            id.clone(),
            observation("message-2"),
            target(),
            UnixTimeMillis::new(1000),
            context,
        ),
        Err(InvalidStoredPropertyChange::ObservationIdentity)
    );
    assert_eq!(
        StoredPropertyChange::new(id, source, target(), UnixTimeMillis::new(1100), context,),
        Err(InvalidStoredPropertyChange::EventTime)
    );
}
