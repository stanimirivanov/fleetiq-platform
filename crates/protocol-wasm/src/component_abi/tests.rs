use std::num::NonZeroUsize;

use fleetiq_domain::IdentifierError;
use fleetiq_protocol_sdk::{
    DecodedMeasurementError, DecodedValue as SdkDecodedValue, FrameBoundary as SdkFrameBoundary,
    FrameProcessingInput, FrameProcessingOutput as SdkFrameProcessingOutput, FramingInput,
    ObservationQuality as SdkObservationQuality, ProposedReplyError,
    ReplyRelease as SdkReplyRelease, SessionStateAction as SdkSessionStateAction,
};

use super::{
    PROTOCOL_PACKAGE_ABI_VERSION, PROTOCOL_PACKAGE_WIT, PROTOCOL_PACKAGE_WORLD,
    bindings::exports::fleetiq::protocol_package::runtime as wit, conversion::ComponentOutputError,
};

#[test]
fn public_identity_matches_the_versioned_wit_package() {
    assert_eq!(PROTOCOL_PACKAGE_ABI_VERSION, "0.1.0");
    assert_eq!(
        PROTOCOL_PACKAGE_WORLD,
        "fleetiq:protocol-package/protocol-package@0.1.0"
    );
    assert!(PROTOCOL_PACKAGE_WIT.contains("package fleetiq:protocol-package@0.1.0;"));
    assert!(PROTOCOL_PACKAGE_WIT.contains("world protocol-package"));
}

#[test]
fn semantic_inputs_are_copied_into_component_owned_values() {
    let framing: wit::InvocationInput = FramingInput::new(b"unread", b"framing-state").into();
    let processing: wit::InvocationInput =
        FrameProcessingInput::new(b"frame", b"processing-state").into();

    assert_eq!(framing.payload, b"unread");
    assert_eq!(framing.session_state, b"framing-state");
    assert_eq!(processing.payload, b"frame");
    assert_eq!(processing.session_state, b"processing-state");
}

#[test]
fn valid_frame_decisions_cross_the_component_boundary() {
    assert_eq!(
        SdkFrameBoundary::try_from(wit::FrameBoundary::NeedMoreData),
        Ok(SdkFrameBoundary::NeedMoreData)
    );
    assert_eq!(
        SdkFrameBoundary::try_from(wit::FrameBoundary::Frame(5)),
        Ok(SdkFrameBoundary::Frame {
            length: NonZeroUsize::new(5).unwrap()
        })
    );
}

#[test]
fn a_zero_component_frame_length_is_rejected() {
    assert_eq!(
        SdkFrameBoundary::try_from(wit::FrameBoundary::Frame(0)),
        Err(ComponentOutputError::ZeroFrameLength)
    );
}

#[test]
fn processing_conversion_preserves_value_state_and_reply_order() {
    let output = wit::FrameProcessingOutput {
        decoded_outputs: vec![
            decoded_output(
                "can.primary",
                "motor.enabled",
                wit::DecodedValue::Boolean(true),
                None,
            ),
            decoded_output(
                "can.primary",
                "motor.speed",
                wit::DecodedValue::Integer(1_720),
                Some("revolutions-per-minute"),
            ),
            decoded_output(
                "battery",
                "battery.voltage",
                wit::DecodedValue::Decimal("67.250".to_owned()),
                Some("volt"),
            ),
            decoded_output(
                "controller",
                "controller.mode",
                wit::DecodedValue::Text("traction".to_owned()),
                None,
            ),
        ],
        session_state: wit::SessionStateAction::Replace(b"identified".to_vec()),
        replies: vec![
            wit::ProposedReply {
                bytes: b"ready".to_vec(),
                release: wit::ReplyRelease::AfterProcessing,
            },
            wit::ProposedReply {
                bytes: b"accepted".to_vec(),
                release: wit::ReplyRelease::AfterDurableAcceptance,
            },
        ],
    };

    let converted = SdkFrameProcessingOutput::try_from(output).unwrap();

    assert_eq!(converted.decoded_outputs().len(), 4);
    assert_eq!(
        converted.decoded_outputs()[0].value(),
        &SdkDecodedValue::Boolean(true)
    );
    assert_eq!(
        converted.decoded_outputs()[1].value(),
        &SdkDecodedValue::Integer(1_720)
    );
    assert_eq!(
        converted.decoded_outputs()[1]
            .source_unit()
            .map(fleetiq_protocol_sdk::UnitId::as_str),
        Some("revolutions-per-minute")
    );
    assert_eq!(
        converted.decoded_outputs()[2].value(),
        &SdkDecodedValue::Decimal("67.250".parse().unwrap())
    );
    assert_eq!(
        converted.decoded_outputs()[3].value(),
        &SdkDecodedValue::Text("traction".to_owned())
    );
    assert_eq!(
        converted.decoded_outputs()[3].quality(),
        SdkObservationQuality::Uncertain
    );
    assert_eq!(
        converted.decoded_outputs()[3]
            .device_recorded_at()
            .map(fleetiq_protocol_sdk::UnixTimeMillis::get),
        Some(1_725_000_000_000)
    );
    assert_eq!(
        converted.session_state_action(),
        &SdkSessionStateAction::Replace(b"identified".to_vec().into_boxed_slice())
    );

    let (_, _, replies) = converted.into_parts();
    let (first_bytes, first_release) = replies[0].clone().into_parts();
    let (second_bytes, second_release) = replies[1].clone().into_parts();
    assert_eq!(&*first_bytes, b"ready");
    assert_eq!(first_release, SdkReplyRelease::AfterProcessing);
    assert_eq!(&*second_bytes, b"accepted");
    assert_eq!(second_release, SdkReplyRelease::AfterDurableAcceptance);
}

#[test]
fn invalid_output_identifiers_report_the_output_position() {
    let output = processing_output(vec![decoded_output(
        "can.primary",
        " bad-signal",
        wit::DecodedValue::Integer(1),
        None,
    )]);

    assert_eq!(
        SdkFrameProcessingOutput::try_from(output),
        Err(ComponentOutputError::InvalidSignalId {
            output_index: 0,
            source: IdentifierError::SurroundingWhitespace,
        })
    );
}

#[test]
fn invalid_decimal_text_reports_the_output_position_without_retaining_the_value() {
    let output = processing_output(vec![decoded_output(
        "can.primary",
        "motor.speed",
        wit::DecodedValue::Decimal("private-invalid-value".to_owned()),
        None,
    )]);

    let error = SdkFrameProcessingOutput::try_from(output).unwrap_err();

    assert_eq!(
        error,
        ComponentOutputError::InvalidDecimal { output_index: 0 }
    );
    assert!(!error.to_string().contains("private-invalid-value"));
}

#[test]
fn a_unit_on_text_is_rejected_through_the_semantic_measurement_constructor() {
    let output = processing_output(vec![decoded_output(
        "controller",
        "controller.mode",
        wit::DecodedValue::Text("traction".to_owned()),
        Some("volt"),
    )]);

    assert_eq!(
        SdkFrameProcessingOutput::try_from(output),
        Err(ComponentOutputError::InvalidMeasurement {
            output_index: 0,
            source: DecodedMeasurementError::UnitRequiresNumericValue,
        })
    );
}

#[test]
fn an_empty_reply_reports_its_ordered_position() {
    let output = wit::FrameProcessingOutput {
        decoded_outputs: Vec::new(),
        session_state: wit::SessionStateAction::Keep,
        replies: vec![
            wit::ProposedReply {
                bytes: b"valid".to_vec(),
                release: wit::ReplyRelease::AfterProcessing,
            },
            wit::ProposedReply {
                bytes: Vec::new(),
                release: wit::ReplyRelease::AfterProcessing,
            },
        ],
    };

    assert_eq!(
        SdkFrameProcessingOutput::try_from(output),
        Err(ComponentOutputError::InvalidReply {
            reply_index: 1,
            source: ProposedReplyError::Empty,
        })
    );
}

fn decoded_output(
    endpoint_id: &str,
    signal_id: &str,
    value: wit::DecodedValue,
    source_unit: Option<&str>,
) -> wit::DecodedOutput {
    wit::DecodedOutput {
        endpoint_id: endpoint_id.to_owned(),
        signal_id: signal_id.to_owned(),
        value,
        source_unit: source_unit.map(str::to_owned),
        device_recorded_at_ms: Some(1_725_000_000_000),
        quality: wit::ObservationQuality::Uncertain,
    }
}

fn processing_output(decoded_outputs: Vec<wit::DecodedOutput>) -> wit::FrameProcessingOutput {
    wit::FrameProcessingOutput {
        decoded_outputs,
        session_state: wit::SessionStateAction::Keep,
        replies: Vec::new(),
    }
}
