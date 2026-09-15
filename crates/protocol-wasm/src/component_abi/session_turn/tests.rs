use std::{convert::Infallible, num::NonZeroUsize};

use fleetiq_protocol_host::{
    FrameProcessingGate, FrameProcessingLimits, FrameProcessingValidationError, SessionInputBuffer,
    SessionState,
};
use fleetiq_protocol_sdk::SessionStateAction;

use super::{ProtocolSessionTurn, ProtocolSessionTurnBudgets, ProtocolSessionTurnError};
use crate::{
    ProtocolComponentInvocationError, ProtocolFrameProcessingError,
    component_abi::test_support::{Fixture, budget, compiled},
};

#[test]
fn one_turn_frames_and_processes_one_prefix_against_the_same_state() {
    let (sandbox, component) = compiled(Fixture::StateLengthFrameAndReplaceState);
    let mut state = state_with_limit(b"old", 8);
    let mut input = buffer(b"abcde");

    let turn = sandbox
        .run_component_session_turn(&component, budgets(), &gate(), &mut state, &mut input)
        .unwrap();

    assert!(turn.framing_report().fuel_consumed() > 0);
    let ProtocolSessionTurn::Processed(processed) = turn else {
        panic!("the state length should identify a complete frame");
    };
    assert_eq!(processed.frame().as_bytes(), b"abc");
    assert_eq!(processed.package(), component.package());
    assert_eq!(processed.artifact_digest(), component.artifact_digest());
    assert!(processed.output().decoded_outputs().is_empty());
    assert!(processed.framing_report().fuel_consumed() > 0);
    assert!(processed.processing_report().fuel_consumed() > 0);
    assert_eq!(state.as_bytes(), b"next");
    assert_eq!(input.as_bytes(), b"de");
    let debug = format!("{processed:?}");
    assert!(!debug.contains("abc"));

    let (frame, package, digest, output, framing_report, processing_report) =
        processed.into_parts();
    assert_eq!(frame.as_bytes(), b"abc");
    assert_eq!(&package, component.package());
    assert_eq!(digest, component.artifact_digest());
    assert!(output.staged_replies().is_empty());
    assert!(framing_report.fuel_consumed() > 0);
    assert!(processing_report.fuel_consumed() > 0);
}

#[test]
fn incomplete_input_ends_the_turn_without_processing_or_mutation() {
    let (sandbox, component) = compiled(Fixture::NeedMoreData);
    let mut state = state_with_limit(b"old", 8);
    let mut input = buffer(b"fragment");

    let turn = sandbox
        .run_component_session_turn(&component, budgets(), &gate(), &mut state, &mut input)
        .unwrap();

    assert!(matches!(
        turn,
        ProtocolSessionTurn::NeedMoreData { framing_report }
            if framing_report.fuel_consumed() > 0
    ));
    assert_eq!(state.as_bytes(), b"old");
    assert_eq!(input.as_bytes(), b"fragment");
}

#[test]
fn framing_failure_preserves_state_and_unread_input() {
    let (sandbox, component) = compiled(Fixture::PackageRejection);
    let mut state = state_with_limit(b"old", 8);
    let mut input = buffer(b"frame");

    let error = sandbox
        .run_component_session_turn(&component, budgets(), &gate(), &mut state, &mut input)
        .unwrap_err();

    assert!(matches!(error, ProtocolSessionTurnError::Framing(_)));
    assert_eq!(state.as_bytes(), b"old");
    assert_eq!(input.as_bytes(), b"frame");
}

#[test]
fn processing_rejection_returns_the_consumed_frame_and_preserves_state() {
    let (sandbox, component) = compiled(Fixture::FrameThenReject);
    let mut state = state_with_limit(b"old", 8);
    let mut input = buffer(b"abcdtail");

    let error = sandbox
        .run_component_session_turn(&component, budgets(), &gate(), &mut state, &mut input)
        .unwrap_err();

    assert!(matches!(
        error,
        ProtocolSessionTurnError::Processing {
            source: ProtocolFrameProcessingError::Invocation(
                ProtocolComponentInvocationError::PackageRejected { code: 42, .. }
            ),
            ref frame,
            framing_report,
        } if frame.as_bytes() == b"abcd" && framing_report.fuel_consumed() > 0
    ));
    assert_eq!(state.as_bytes(), b"old");
    assert_eq!(input.as_bytes(), b"tail");
}

#[test]
fn host_validation_failure_returns_frame_and_does_not_apply_state() {
    let (sandbox, component) = compiled(Fixture::StateLengthFrameAndReplaceState);
    let mut state = state_with_limit(b"old", 3);
    let mut input = buffer(b"abcnext");

    let error = sandbox
        .run_component_session_turn(&component, budgets(), &gate(), &mut state, &mut input)
        .unwrap_err();

    assert!(matches!(
        error,
        ProtocolSessionTurnError::Processing {
            source: ProtocolFrameProcessingError::Validation {
                source: FrameProcessingValidationError::SessionStateLimitExceeded {
                    limit: 3,
                    attempted: 4,
                },
                report,
            },
            ref frame,
            framing_report,
        } if frame.as_bytes() == b"abc"
            && framing_report.fuel_consumed() > 0
            && report.fuel_consumed() > 0
    ));
    assert_eq!(state.as_bytes(), b"old");
    assert_eq!(input.as_bytes(), b"next");
}

fn budgets() -> ProtocolSessionTurnBudgets {
    ProtocolSessionTurnBudgets::new(budget(), budget())
}

fn state_with_limit(initial: &[u8], limit: usize) -> SessionState {
    let mut state = SessionState::new(nonzero(limit));
    state
        .update(|_| {
            Ok::<_, Infallible>(SessionStateAction::Replace(
                initial.to_vec().into_boxed_slice(),
            ))
        })
        .unwrap();
    state
}

fn buffer(bytes: &[u8]) -> SessionInputBuffer {
    let mut input = SessionInputBuffer::new(nonzero(32));
    input.push(bytes).unwrap();
    input
}

fn gate() -> FrameProcessingGate {
    FrameProcessingGate::new(FrameProcessingLimits::new(
        nonzero(8),
        nonzero(4),
        nonzero(32),
        nonzero(64),
    ))
}

fn nonzero(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("test value is nonzero")
}
