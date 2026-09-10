use std::str::FromStr;

use bigdecimal::BigDecimal;
use fleetiq_domain::{
    AssetId, DeviceEndpointId, PropertyValueKind, SignalSource, SourceSignalId, TenantId, UnitId,
    UnixTimeMillis,
};

use super::*;
use crate::{AcceptedMessageId, ProtocolPackageId, ProtocolPackageVersion};

fn observation(
    value: DecodedValue,
    source_unit: Option<UnitId>,
) -> Result<DecodedObservation, DecodedMeasurementError> {
    let measurement = DecodedMeasurement::new(value, source_unit, ObservationQuality::Uncertain)?;
    Ok(DecodedObservation::new(
        DecodedObservationId::new(
            AcceptedMessageId::new("message-100").expect("valid message ID"),
            ProtocolPackageRef::new(
                ProtocolPackageId::new("example.binary").expect("valid package ID"),
                ProtocolPackageVersion::new("2.1.0").expect("valid package version"),
            ),
            3,
        ),
        TenantId::new("tenant-a").expect("valid tenant ID"),
        SignalSource::new(
            AssetId::new("gateway-1").expect("valid asset ID"),
            DeviceEndpointId::new("can-primary").expect("valid endpoint ID"),
            SourceSignalId::new("bus.voltage").expect("valid signal ID"),
        ),
        measurement,
        ObservationTimes::new(Some(UnixTimeMillis::new(1_000)), UnixTimeMillis::new(1_200)),
    ))
}

#[test]
fn preserves_source_value_times_quality_and_provenance() {
    let decoded = observation(
        DecodedValue::Decimal(BigDecimal::from_str("48.125").expect("valid decimal")),
        Some(UnitId::new("volt").expect("valid unit ID")),
    )
    .expect("observation should be valid");

    assert_eq!(decoded.id().message_id().as_str(), "message-100");
    assert_eq!(decoded.id().output_index(), 3);
    assert_eq!(decoded.tenant_id().as_str(), "tenant-a");
    assert_eq!(decoded.source().device_asset_id().as_str(), "gateway-1");
    assert_eq!(decoded.source().endpoint_id().as_str(), "can-primary");
    assert_eq!(decoded.source().signal_id().as_str(), "bus.voltage");
    assert_eq!(decoded.value().kind(), PropertyValueKind::Decimal);
    assert_eq!(decoded.source_unit().map(UnitId::as_str), Some("volt"));
    assert_eq!(decoded.quality(), ObservationQuality::Uncertain);
    assert_eq!(
        decoded.device_recorded_at(),
        Some(UnixTimeMillis::new(1_000))
    );
    assert_eq!(decoded.received_at(), UnixTimeMillis::new(1_200));
    assert_eq!(decoded.decoded_by().id().as_str(), "example.binary");
    assert_eq!(decoded.decoded_by().version().as_str(), "2.1.0");
}

#[test]
fn stable_identity_distinguishes_outputs_from_one_message() {
    let message_id = AcceptedMessageId::new("message-100").expect("valid message ID");

    assert_ne!(
        DecodedObservationId::new(
            message_id.clone(),
            ProtocolPackageRef::new(
                ProtocolPackageId::new("example.binary").expect("valid package ID"),
                ProtocolPackageVersion::new("1").expect("valid package version"),
            ),
            0,
        ),
        DecodedObservationId::new(
            message_id,
            ProtocolPackageRef::new(
                ProtocolPackageId::new("example.binary").expect("valid package ID"),
                ProtocolPackageVersion::new("1").expect("valid package version"),
            ),
            1,
        )
    );
}

#[test]
fn reinterpreting_a_message_with_another_package_release_has_a_new_identity() {
    let message_id = AcceptedMessageId::new("message-100").expect("valid message ID");
    let package_id = ProtocolPackageId::new("example.binary").expect("valid package ID");

    assert_ne!(
        DecodedObservationId::new(
            message_id.clone(),
            ProtocolPackageRef::new(
                package_id.clone(),
                ProtocolPackageVersion::new("1").expect("valid package version"),
            ),
            0,
        ),
        DecodedObservationId::new(
            message_id,
            ProtocolPackageRef::new(
                package_id,
                ProtocolPackageVersion::new("2").expect("valid package version"),
            ),
            0,
        )
    );
}

#[test]
fn accepts_units_only_for_numeric_values() {
    let unit = || UnitId::new("volt").expect("valid unit ID");

    assert!(observation(DecodedValue::Integer(48), Some(unit())).is_ok());
    assert_eq!(
        observation(DecodedValue::Boolean(true), Some(unit())),
        Err(DecodedMeasurementError::UnitRequiresNumericValue)
    );
    assert_eq!(
        observation(DecodedValue::Text("online".to_owned()), Some(unit())),
        Err(DecodedMeasurementError::UnitRequiresNumericValue)
    );
}

#[test]
fn keeps_device_and_receipt_times_independent() {
    let decoded = DecodedObservation::new(
        DecodedObservationId::new(
            AcceptedMessageId::new("message-clock-skew").expect("valid message ID"),
            ProtocolPackageRef::new(
                ProtocolPackageId::new("example.clock").expect("valid package ID"),
                ProtocolPackageVersion::new("1").expect("valid package version"),
            ),
            0,
        ),
        TenantId::new("tenant-a").expect("valid tenant ID"),
        SignalSource::new(
            AssetId::new("gateway-1").expect("valid asset ID"),
            DeviceEndpointId::new("onboard").expect("valid endpoint ID"),
            SourceSignalId::new("clock.sample").expect("valid signal ID"),
        ),
        DecodedMeasurement::new(DecodedValue::Integer(1), None, ObservationQuality::Good)
            .expect("measurement should be valid"),
        ObservationTimes::new(Some(UnixTimeMillis::new(2_000)), UnixTimeMillis::new(1_000)),
    );

    assert_eq!(
        decoded.device_recorded_at(),
        Some(UnixTimeMillis::new(2_000))
    );
    assert_eq!(decoded.received_at(), UnixTimeMillis::new(1_000));
}
