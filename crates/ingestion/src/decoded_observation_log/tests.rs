use fleetiq_domain::{
    AssetId, DeviceEndpointId, PropertyValue, SignalSource, SourceSignalId, TenantId,
    UnixTimeMillis,
};
use fleetiq_telemetry::{
    AcceptedMessage, AcceptedMessageId, AcceptedPayload, DecodedMeasurement, DecodedObservation,
    ObservationQuality, ObservationTimes, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};

use super::*;

#[test]
fn accepts_complete_ordered_observation_batch() {
    let message = accepted_message();
    let observations = vec![observation(&message, 0), observation(&message, 1)];
    let batch = DecodedObservationBatch::new(message, observations).expect("valid batch");

    assert_eq!(batch.len(), 2);
    assert!(!batch.is_empty());
    assert_eq!(batch.observations()[1].id().output_index(), 1);
    assert!(DecodedObservationAppendOutcome::Appended.was_appended());
    assert!(!DecodedObservationAppendOutcome::AlreadyPresent.was_appended());
}

#[test]
fn accepts_empty_batch_as_completed_decoding() {
    let batch = DecodedObservationBatch::new(accepted_message(), Vec::new())
        .expect("empty decoding result is valid");

    assert!(batch.is_empty());
    assert_eq!(batch.message().id().as_str(), "message-1");
}

#[test]
fn debug_omits_payload_and_observation_values() {
    let message = accepted_message();
    let batch = DecodedObservationBatch::new(message.clone(), vec![observation(&message, 0)])
        .expect("valid batch");
    let debug = format!("{batch:?}");

    assert!(debug.contains("observation_count: 1"));
    assert!(!debug.contains("private-wire-frame"));
    assert!(!debug.contains("private-value"));
}

#[test]
fn rejects_noncontiguous_output_identity() {
    let message = accepted_message();
    let error = DecodedObservationBatch::new(message.clone(), vec![observation(&message, 2)])
        .expect_err("position and output identity must agree");

    assert_eq!(
        error,
        DecodedObservationBatchError::OutputIndexMismatch {
            index: 0,
            actual: 2,
        }
    );
}

#[test]
fn rejects_mismatched_trusted_provenance() {
    let message = accepted_message();

    assert_batch_error(
        &message,
        ObservationOverrides {
            tenant_id: "tenant-b",
            ..ObservationOverrides::default()
        },
        DecodedObservationBatchError::TenantMismatch { index: 0 },
    );
    assert_batch_error(
        &message,
        ObservationOverrides {
            device_id: "device-b",
            ..ObservationOverrides::default()
        },
        DecodedObservationBatchError::DeviceMismatch { index: 0 },
    );
    assert_batch_error(
        &message,
        ObservationOverrides {
            message_id: "message-2",
            ..ObservationOverrides::default()
        },
        DecodedObservationBatchError::MessageMismatch { index: 0 },
    );
    assert_batch_error(
        &message,
        ObservationOverrides {
            package_version: "2",
            ..ObservationOverrides::default()
        },
        DecodedObservationBatchError::ProtocolPackageMismatch { index: 0 },
    );
    assert_batch_error(
        &message,
        ObservationOverrides {
            received_at: 2_000,
            ..ObservationOverrides::default()
        },
        DecodedObservationBatchError::ReceivedAtMismatch { index: 0 },
    );
}

fn assert_batch_error(
    message: &AcceptedMessage,
    overrides: ObservationOverrides<'_>,
    expected: DecodedObservationBatchError,
) {
    let error = DecodedObservationBatch::new(message.clone(), vec![observation_with(0, overrides)])
        .expect_err("mismatched provenance must fail");

    assert_eq!(error, expected);
}

#[derive(Clone, Copy)]
struct ObservationOverrides<'a> {
    tenant_id: &'a str,
    device_id: &'a str,
    message_id: &'a str,
    package_version: &'a str,
    received_at: i64,
}

impl Default for ObservationOverrides<'static> {
    fn default() -> Self {
        Self {
            tenant_id: "tenant-a",
            device_id: "device-a",
            message_id: "message-1",
            package_version: "1",
            received_at: 1_000,
        }
    }
}

fn accepted_message() -> AcceptedMessage {
    AcceptedMessage::new(
        AcceptedMessageId::new("message-1").expect("valid message ID"),
        TenantId::new("tenant-a").expect("valid tenant ID"),
        AssetId::new("device-a").expect("valid asset ID"),
        package("1"),
        UnixTimeMillis::new(1_000),
        AcceptedPayload::new(b"private-wire-frame".as_slice()),
    )
}

fn observation(message: &AcceptedMessage, output_index: u32) -> DecodedObservation {
    DecodedObservation::new(
        message.decoded_observation_id(output_index),
        message.tenant_id().clone(),
        SignalSource::new(
            message.device_asset_id().clone(),
            DeviceEndpointId::new("can-1").expect("valid endpoint ID"),
            SourceSignalId::new("freezer.temperature").expect("valid signal ID"),
        ),
        measurement(),
        ObservationTimes::new(None, message.received_at()),
    )
}

fn observation_with(output_index: u32, overrides: ObservationOverrides<'_>) -> DecodedObservation {
    DecodedObservation::new(
        fleetiq_telemetry::DecodedObservationId::new(
            AcceptedMessageId::new(overrides.message_id).expect("valid message ID"),
            package(overrides.package_version),
            output_index,
        ),
        TenantId::new(overrides.tenant_id).expect("valid tenant ID"),
        SignalSource::new(
            AssetId::new(overrides.device_id).expect("valid asset ID"),
            DeviceEndpointId::new("can-1").expect("valid endpoint ID"),
            SourceSignalId::new("freezer.temperature").expect("valid signal ID"),
        ),
        measurement(),
        ObservationTimes::new(None, UnixTimeMillis::new(overrides.received_at)),
    )
}

fn measurement() -> DecodedMeasurement {
    DecodedMeasurement::new(
        PropertyValue::Text("private-value".to_owned()),
        None,
        ObservationQuality::Good,
    )
    .expect("valid measurement")
}

fn package(version: &str) -> ProtocolPackageRef {
    ProtocolPackageRef::new(
        ProtocolPackageId::new("example.protocol").expect("valid package ID"),
        ProtocolPackageVersion::new(version).expect("valid package version"),
    )
}
