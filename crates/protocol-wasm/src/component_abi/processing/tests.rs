use std::{convert::Infallible, num::NonZeroUsize};

use fleetiq_protocol_host::{
    FramePoll, FrameProcessingGate, FrameProcessingLimits, FrameProcessingValidationError,
    SessionInputBuffer, SessionState, WireFrame,
};
use fleetiq_protocol_sdk::{FrameBoundary, SessionStateAction};

use super::ProtocolFrameProcessingError;
use crate::{
    ProtocolComponentInvocationError,
    component_abi::test_support::{Fixture, budget, compiled},
};

#[test]
fn valid_component_output_is_applied_to_the_same_session_state() {
    let (sandbox, component) = compiled(Fixture::ReplaceState);
    let mut state = state_with_limit(b"old", 8);
    let frame = frame(b"frame");

    let processed = sandbox
        .process_component_frame(&component, budget(), &gate(), &mut state, &frame)
        .unwrap();

    assert_eq!(state.as_bytes(), b"next");
    assert_eq!(processed.output().decoded_outputs().len(), 0);
    assert_eq!(processed.output().staged_replies().len(), 0);
    assert!(processed.report().fuel_consumed() > 0);
    assert_eq!(processed.package(), component.package());
    assert_eq!(processed.artifact_digest(), component.artifact_digest());
    let (package, digest, output, report) = processed.into_parts();
    assert_eq!(&package, component.package());
    assert_eq!(digest, component.artifact_digest());
    assert_eq!(output.decoded_outputs().len(), 0);
    assert!(report.fuel_consumed() > 0);
}

#[test]
fn host_validation_failure_preserves_state_and_reports_invocation_usage() {
    let (sandbox, component) = compiled(Fixture::ReplaceState);
    let mut state = state_with_limit(b"old", 3);
    let frame = frame(b"frame");

    let error = sandbox
        .process_component_frame(&component, budget(), &gate(), &mut state, &frame)
        .unwrap_err();

    assert_eq!(state.as_bytes(), b"old");
    assert!(matches!(
        error,
        ProtocolFrameProcessingError::Validation {
            source: FrameProcessingValidationError::SessionStateLimitExceeded {
                limit: 3,
                attempted: 4,
            },
            report,
        } if report.fuel_consumed() > 0
    ));
}

#[test]
fn package_rejection_preserves_state_and_bypasses_the_host_gate() {
    let (sandbox, component) = compiled(Fixture::PackageRejection);
    let mut state = state_with_limit(b"old", 8);
    let frame = frame(b"frame");

    let error = sandbox
        .process_component_frame(&component, budget(), &gate(), &mut state, &frame)
        .unwrap_err();

    assert_eq!(state.as_bytes(), b"old");
    assert!(matches!(
        error,
        ProtocolFrameProcessingError::Invocation(
            ProtocolComponentInvocationError::PackageRejected {
                code: 42,
                report,
            }
        ) if report.fuel_consumed() > 0
    ));
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

fn frame(bytes: &[u8]) -> WireFrame {
    let mut input = SessionInputBuffer::new(nonzero(bytes.len()));
    input.push(bytes).unwrap();
    match input
        .poll_frame(|_| {
            Ok::<_, Infallible>(FrameBoundary::Frame {
                length: nonzero(bytes.len()),
            })
        })
        .unwrap()
    {
        FramePoll::Frame(frame) => frame,
        FramePoll::NeedMoreData => panic!("complete fixture frame was not extracted"),
    }
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
