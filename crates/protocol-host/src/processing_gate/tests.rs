use std::{convert::Infallible, num::NonZeroUsize};

use fleetiq_protocol_sdk::{
    DecodedMeasurement, DecodedOutput, DecodedValue, DeviceEndpointId, FrameProcessingOutput,
    ObservationQuality, ProposedReply, ReplyRelease, SessionStateAction, SourceSignalId,
};

use super::{FrameProcessingGate, FrameProcessingLimits, FrameProcessingValidationError};
use crate::{ReplyStagingError, SessionState, StagedReply};

fn limit(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("test limits are nonzero")
}

fn gate(
    max_outputs: usize,
    max_replies: usize,
    max_reply_bytes: usize,
    max_total_reply_bytes: usize,
) -> FrameProcessingGate {
    FrameProcessingGate::new(FrameProcessingLimits::new(
        limit(max_outputs),
        limit(max_replies),
        limit(max_reply_bytes),
        limit(max_total_reply_bytes),
    ))
}

fn state(limit_bytes: usize, initial: &[u8]) -> SessionState {
    let mut state = SessionState::new(limit(limit_bytes));
    state
        .update(|_| {
            Ok::<_, Infallible>(SessionStateAction::Replace(
                initial.to_vec().into_boxed_slice(),
            ))
        })
        .expect("initial test state fits");
    state
}

fn decoded_output(signal: &str, value: i64) -> DecodedOutput {
    DecodedOutput::new(
        DeviceEndpointId::new("can.primary").expect("valid test endpoint ID"),
        SourceSignalId::new(signal).expect("valid test signal ID"),
        DecodedMeasurement::new(DecodedValue::Integer(value), None, ObservationQuality::Good)
            .expect("valid test measurement"),
        None,
    )
}

fn reply(bytes: &[u8], release: ReplyRelease) -> ProposedReply {
    ProposedReply::new(bytes, release).expect("test reply is nonempty")
}

#[test]
fn valid_result_applies_state_and_preserves_output_and_reply_order() {
    let gate = gate(2, 2, 8, 12);
    let mut state = state(16, b"old");
    let first_output = decoded_output("motor.speed", 1_720);
    let second_output = decoded_output("battery.voltage", 672);

    let applied = gate
        .apply(
            &mut state,
            FrameProcessingOutput::new(
                vec![first_output.clone(), second_output.clone()],
                SessionStateAction::Replace(b"active".to_vec().into_boxed_slice()),
                vec![
                    reply(b"ready", ReplyRelease::AfterProcessing),
                    reply(b"later", ReplyRelease::AfterDurableAcceptance),
                ],
            ),
        )
        .expect("result satisfies every host limit");

    assert_eq!(state.as_bytes(), b"active");
    assert_eq!(applied.decoded_outputs(), [first_output, second_output]);
    assert!(matches!(applied.staged_replies()[0], StagedReply::Ready(_)));
    assert!(matches!(
        applied.staged_replies()[1],
        StagedReply::AwaitingDurableAcceptance(_)
    ));

    let (outputs, replies) = applied.into_parts();
    assert_eq!(outputs.len(), 2);
    assert_eq!(replies.len(), 2);
}

#[test]
fn decoded_output_count_failure_preserves_session_state() {
    let gate = gate(1, 1, 8, 8);
    let mut state = state(8, b"old");

    let error = gate
        .apply(
            &mut state,
            FrameProcessingOutput::new(
                vec![decoded_output("a", 1), decoded_output("b", 2)],
                SessionStateAction::Replace(b"new".to_vec().into_boxed_slice()),
                Vec::new(),
            ),
        )
        .expect_err("two outputs exceed the configured limit");

    assert_eq!(
        error,
        FrameProcessingValidationError::DecodedOutputLimitExceeded {
            limit: 1,
            attempted: 2,
        }
    );
    assert_eq!(state.as_bytes(), b"old");
}

#[test]
fn reply_count_failure_preserves_session_state() {
    let gate = gate(1, 1, 8, 16);
    let mut state = state(8, b"old");

    let error = gate
        .apply(
            &mut state,
            FrameProcessingOutput::new(
                Vec::new(),
                SessionStateAction::Replace(b"new".to_vec().into_boxed_slice()),
                vec![
                    reply(b"a", ReplyRelease::AfterProcessing),
                    reply(b"b", ReplyRelease::AfterProcessing),
                ],
            ),
        )
        .expect_err("two replies exceed the configured count");

    assert_eq!(
        error,
        FrameProcessingValidationError::ReplyCountLimitExceeded {
            limit: 1,
            attempted: 2,
        }
    );
    assert_eq!(state.as_bytes(), b"old");
}

#[test]
fn total_reply_bytes_failure_preserves_session_state() {
    let gate = gate(1, 2, 5, 7);
    let mut state = state(8, b"old");

    let error = gate
        .apply(
            &mut state,
            FrameProcessingOutput::new(
                Vec::new(),
                SessionStateAction::Replace(b"new".to_vec().into_boxed_slice()),
                vec![
                    reply(b"four", ReplyRelease::AfterProcessing),
                    reply(b"more", ReplyRelease::AfterProcessing),
                ],
            ),
        )
        .expect_err("combined reply bytes exceed the frame limit");

    assert_eq!(
        error,
        FrameProcessingValidationError::ReplyBytesLimitExceeded {
            limit: 7,
            attempted: 8,
        }
    );
    assert_eq!(state.as_bytes(), b"old");
}

#[test]
fn individual_reply_failure_reports_its_ordered_index() {
    let gate = gate(1, 2, 4, 16);
    let mut state = state(8, b"old");

    let error = gate
        .apply(
            &mut state,
            FrameProcessingOutput::new(
                Vec::new(),
                SessionStateAction::Replace(b"new".to_vec().into_boxed_slice()),
                vec![
                    reply(b"fine", ReplyRelease::AfterProcessing),
                    reply(b"large", ReplyRelease::AfterProcessing),
                ],
            ),
        )
        .expect_err("the second reply exceeds the individual limit");

    assert_eq!(
        error,
        FrameProcessingValidationError::Reply {
            index: 1,
            source: ReplyStagingError::LimitExceeded {
                limit: 4,
                attempted: 5,
            },
        }
    );
    assert_eq!(state.as_bytes(), b"old");
}

#[test]
fn state_failure_does_not_expose_already_staged_replies() {
    let gate = gate(1, 1, 8, 8);
    let mut state = state(3, b"old");

    let error = gate
        .apply(
            &mut state,
            FrameProcessingOutput::new(
                Vec::new(),
                SessionStateAction::Replace(b"large".to_vec().into_boxed_slice()),
                vec![reply(b"ready", ReplyRelease::AfterProcessing)],
            ),
        )
        .expect_err("the replacement exceeds the session limit");

    assert_eq!(
        error,
        FrameProcessingValidationError::SessionStateLimitExceeded {
            limit: 3,
            attempted: 5,
        }
    );
    assert_eq!(state.as_bytes(), b"old");
}

#[test]
fn exact_processing_limits_are_accepted_and_debug_hides_values() {
    let gate = gate(1, 1, 4, 4);
    let mut state = SessionState::new(limit(3));

    let applied = gate
        .apply(
            &mut state,
            FrameProcessingOutput::new(
                vec![decoded_output("private.signal", 42)],
                SessionStateAction::Replace(b"new".to_vec().into_boxed_slice()),
                vec![reply(b"hide", ReplyRelease::AfterProcessing)],
            ),
        )
        .expect("values exactly at every limit are accepted");

    assert_eq!(state.as_bytes(), b"new");
    let debug = format!("{applied:?}");
    assert!(debug.contains("decoded_output_count: 1"));
    assert!(debug.contains("staged_reply_count: 1"));
    assert!(!debug.contains("private"));
    assert!(!debug.contains("hide"));
    assert!(!debug.contains("42"));
}
