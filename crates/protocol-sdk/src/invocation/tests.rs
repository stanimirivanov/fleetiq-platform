use crate::{
    DecodedMeasurement, DecodedOutput, DecodedValue, DeviceEndpointId, FrameProcessingInput,
    FrameProcessingOutput, FramingInput, ObservationQuality, ProposedReply, ReplyRelease,
    SessionStateAction, SourceSignalId,
};

#[test]
fn framing_input_borrows_bytes_without_exposing_them_in_debug_output() {
    let input = FramingInput::new(b"secret-frame", b"secret-state");

    assert_eq!(input.unread(), b"secret-frame");
    assert_eq!(input.session_state(), b"secret-state");

    let debug = format!("{input:?}");
    assert!(debug.contains("unread_length: 12"));
    assert!(debug.contains("session_state_length: 12"));
    assert!(!debug.contains("secret"));
}

#[test]
fn processing_input_borrows_one_frame_and_current_state() {
    let input = FrameProcessingInput::new(b"frame", b"state");

    assert_eq!(input.frame(), b"frame");
    assert_eq!(input.session_state(), b"state");

    let debug = format!("{input:?}");
    assert!(debug.contains("frame_length: 5"));
    assert!(debug.contains("session_state_length: 5"));
    assert!(!debug.contains("frame: ["));
    assert!(!debug.contains("state: ["));
}

#[test]
fn processing_output_preserves_order_and_complete_state_action() {
    let first_output = decoded_output("can.1", "motor.speed", 1_720);
    let second_output = decoded_output("can.2", "battery.voltage", 672);
    let first_reply = reply([0x01], ReplyRelease::AfterProcessing);
    let second_reply = reply([0x02], ReplyRelease::AfterDurableAcceptance);
    let output = FrameProcessingOutput::new(
        vec![first_output.clone(), second_output.clone()],
        SessionStateAction::Replace(vec![0xaa, 0xbb].into_boxed_slice()),
        vec![first_reply.clone(), second_reply.clone()],
    );

    assert_eq!(output.decoded_outputs(), [first_output, second_output]);
    assert_eq!(output.replies(), [first_reply, second_reply]);
    assert_eq!(
        output.session_state_action(),
        &SessionStateAction::Replace(vec![0xaa, 0xbb].into_boxed_slice())
    );

    let (decoded_outputs, state_action, replies) = output.into_parts();
    assert_eq!(decoded_outputs.len(), 2);
    assert_eq!(replies.len(), 2);
    assert_eq!(
        state_action,
        SessionStateAction::Replace(vec![0xaa, 0xbb].into_boxed_slice())
    );
}

#[test]
fn processing_output_allows_a_frame_without_telemetry_or_replies() {
    let output = FrameProcessingOutput::new(Vec::new(), SessionStateAction::Keep, Vec::new());

    assert_eq!(output.decoded_outputs().len(), 0);
    assert_eq!(output.replies().len(), 0);
    assert_eq!(output.session_state_action(), &SessionStateAction::Keep);
}

#[test]
fn processing_output_debug_reports_shape_without_package_payloads() {
    let output = FrameProcessingOutput::new(
        vec![decoded_output("private.endpoint", "private.signal", 42)],
        SessionStateAction::Replace(b"private-state".to_vec().into_boxed_slice()),
        vec![reply(
            b"private-reply",
            ReplyRelease::AfterDurableAcceptance,
        )],
    );

    let debug = format!("{output:?}");
    assert!(debug.contains("decoded_output_count: 1"));
    assert!(debug.contains("length: 13"));
    assert!(debug.contains("reply_count: 1"));
    assert!(!debug.contains("private"));
    assert!(!debug.contains("42"));
}

fn decoded_output(endpoint: &str, signal: &str, value: i64) -> DecodedOutput {
    DecodedOutput::new(
        DeviceEndpointId::new(endpoint).expect("valid test endpoint ID"),
        SourceSignalId::new(signal).expect("valid test signal ID"),
        DecodedMeasurement::new(DecodedValue::Integer(value), None, ObservationQuality::Good)
            .expect("valid test measurement"),
        None,
    )
}

fn reply(bytes: impl Into<Vec<u8>>, release: ReplyRelease) -> ProposedReply {
    ProposedReply::new(bytes, release).expect("nonempty test reply")
}
