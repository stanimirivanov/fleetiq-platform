use fleetiq_domain::{
    AssetId, DefinitionVersion, DeviceEndpointId, PropertyDefinitionId, PropertyDefinitionRef,
    RecordRevision, SignalBindingId, SignalSource, SignalTarget, SourceSignalId, TenantId, UnitId,
    UnixTimeMillis,
};
use fleetiq_telemetry::{
    AcceptedMessageId, DecodedMeasurement, DecodedObservation, DecodedObservationId, DecodedValue,
    ObservationQuality, ObservationTimes, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};

use super::{AssetPropertyChange, AssetPropertyChangeError, EventTimeBasis};
use crate::{BoundObservation, ResolutionContext, SignalBindingProvenance};

#[test]
fn selects_device_time_without_losing_semantic_or_source_lineage() {
    let bound = bound_observation(Some(instant(1_000)));

    let change = AssetPropertyChange::new(bound, EventTimeBasis::DeviceRecorded)
        .expect("device time should be available");

    assert_eq!(change.event_at(), instant(1_000));
    assert_eq!(change.event_time_basis(), EventTimeBasis::DeviceRecorded);
    assert_eq!(
        change.id().observation_id(),
        change.bound_observation().id()
    );
    assert_eq!(change.id().binding_id().as_str(), "binding-1");
    assert_eq!(change.id().binding_revision().get(), 3);
    assert_eq!(change.tenant_id().as_str(), "tenant-a");
    assert_eq!(change.target().asset_id().as_str(), "motor-1");
    assert_eq!(change.target().property().id().as_str(), "rotational.speed");
    assert_eq!(change.value(), &DecodedValue::Integer(1_720));
    assert_eq!(change.unit().map(UnitId::as_str), Some("rpm"));
    assert_eq!(change.quality(), ObservationQuality::Good);
    assert_eq!(
        change.bound_observation().binding().binding_id().as_str(),
        "binding-1"
    );
}

#[test]
fn receipt_time_remains_an_explicit_choice_when_device_time_exists() {
    let bound = bound_observation(Some(instant(1_000)));

    let change = AssetPropertyChange::new(bound, EventTimeBasis::ReceivedAt)
        .expect("receipt time should always be available");

    assert_eq!(change.event_at(), instant(1_100));
    assert_eq!(change.event_time_basis(), EventTimeBasis::ReceivedAt);
    assert_eq!(
        change
            .bound_observation()
            .observation()
            .device_recorded_at(),
        Some(instant(1_000))
    );
}

#[test]
fn rejects_device_time_selection_when_the_evidence_is_absent() {
    let result = AssetPropertyChange::new(bound_observation(None), EventTimeBasis::DeviceRecorded);

    assert_eq!(
        result,
        Err(AssetPropertyChangeError::MissingDeviceRecordedTime)
    );
}

#[test]
fn receipt_time_is_available_without_a_device_timestamp() {
    let change = AssetPropertyChange::new(bound_observation(None), EventTimeBasis::ReceivedAt)
        .expect("receipt time should always be available");

    assert_eq!(change.event_at(), instant(1_100));
    assert_eq!(
        change
            .bound_observation()
            .observation()
            .device_recorded_at(),
        None
    );
}

#[test]
fn corrected_binding_revision_produces_a_distinct_change_identity() {
    let original = AssetPropertyChange::new(
        bound_observation_with_revision(Some(instant(1_000)), 3),
        EventTimeBasis::DeviceRecorded,
    )
    .expect("device time should be available");
    let corrected = AssetPropertyChange::new(
        bound_observation_with_revision(Some(instant(1_000)), 4),
        EventTimeBasis::DeviceRecorded,
    )
    .expect("device time should be available");

    assert_ne!(original.id(), corrected.id());
    assert_eq!(
        original.id().observation_id(),
        corrected.id().observation_id()
    );
}

fn bound_observation(device_recorded_at: Option<UnixTimeMillis>) -> BoundObservation {
    bound_observation_with_revision(device_recorded_at, 3)
}

fn bound_observation_with_revision(
    device_recorded_at: Option<UnixTimeMillis>,
    binding_revision: u64,
) -> BoundObservation {
    let package = ProtocolPackageRef::new(
        ProtocolPackageId::new("example.protocol").expect("valid package ID"),
        ProtocolPackageVersion::new("1").expect("valid package version"),
    );
    let observation = DecodedObservation::new(
        DecodedObservationId::new(
            AcceptedMessageId::new("message-1").expect("valid message ID"),
            package,
            0,
        ),
        TenantId::new("tenant-a").expect("valid tenant ID"),
        SignalSource::new(
            AssetId::new("gateway-1").expect("valid device asset ID"),
            DeviceEndpointId::new("can-primary").expect("valid endpoint ID"),
            SourceSignalId::new("motor.speed").expect("valid signal ID"),
        ),
        DecodedMeasurement::new(
            DecodedValue::Integer(1_720),
            Some(UnitId::new("rpm").expect("valid unit ID")),
            ObservationQuality::Good,
        )
        .expect("numeric measurement with a unit should be valid"),
        ObservationTimes::new(device_recorded_at, instant(1_100)),
    );

    BoundObservation::new(
        observation,
        SignalTarget::new(
            AssetId::new("motor-1").expect("valid target asset ID"),
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new("rotational.speed").expect("valid property ID"),
                DefinitionVersion::new(2).expect("valid definition version"),
            ),
        ),
        SignalBindingProvenance::new(
            SignalBindingId::new("binding-1").expect("valid binding ID"),
            RecordRevision::new(binding_revision).expect("valid binding revision"),
        ),
        ResolutionContext::new(instant(1_000), instant(1_200)),
    )
}

const fn instant(value: i64) -> UnixTimeMillis {
    UnixTimeMillis::new(value)
}
