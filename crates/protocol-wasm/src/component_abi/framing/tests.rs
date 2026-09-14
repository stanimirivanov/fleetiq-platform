use std::{convert::Infallible, num::NonZeroUsize};

use fleetiq_protocol_host::{FramePoll, SessionInputBuffer, SessionState};
use fleetiq_protocol_sdk::SessionStateAction;

use super::ProtocolFramePollError;
use crate::{
    ProtocolComponentInvocationError,
    component_abi::test_support::{Fixture, budget, compiled},
};

#[test]
fn current_session_state_drives_framing_and_only_the_frame_prefix_is_removed() {
    let (sandbox, component) = compiled(Fixture::StateLengthFrame);
    let state = state(b"xyz");
    let mut input = buffer(b"abcde");

    let polled = sandbox
        .poll_component_frame(&component, budget(), &state, &mut input)
        .unwrap();

    assert!(polled.report().fuel_consumed() > 0);
    let debug = format!("{polled:?}");
    assert!(!debug.contains("abc"));
    let (outcome, report) = polled.into_parts();
    let FramePoll::Frame(frame) = outcome else {
        panic!("state length should identify one complete frame");
    };
    assert_eq!(frame.as_bytes(), b"abc");
    assert_eq!(input.as_bytes(), b"de");
    assert!(report.fuel_consumed() > 0);
}

#[test]
fn need_more_data_preserves_all_unread_input() {
    let (sandbox, component) = compiled(Fixture::NeedMoreData);
    let state = state(&[]);
    let mut input = buffer(b"fragment");

    let polled = sandbox
        .poll_component_frame(&component, budget(), &state, &mut input)
        .unwrap();

    assert_eq!(polled.outcome(), &FramePoll::NeedMoreData);
    assert_eq!(input.as_bytes(), b"fragment");
    assert!(polled.report().fuel_consumed() > 0);
}

#[test]
fn package_rejection_preserves_all_unread_input() {
    let (sandbox, component) = compiled(Fixture::PackageRejection);
    let state = state(b"state");
    let mut input = buffer(b"private-frame");

    let error = sandbox
        .poll_component_frame(&component, budget(), &state, &mut input)
        .unwrap_err();

    assert_eq!(input.as_bytes(), b"private-frame");
    assert!(matches!(
        error,
        ProtocolFramePollError::Invocation(
            ProtocolComponentInvocationError::PackageRejected {
                code: 42,
                report,
            }
        ) if report.fuel_consumed() > 0
    ));
}

#[test]
fn frame_longer_than_the_inspected_buffer_is_rejected_without_consumption() {
    let (sandbox, component) = compiled(Fixture::OversizedFrame);
    let state = state(&[]);
    let mut input = buffer(b"abc");

    let error = sandbox
        .poll_component_frame(&component, budget(), &state, &mut input)
        .unwrap_err();

    assert_eq!(input.as_bytes(), b"abc");
    assert!(matches!(
        error,
        ProtocolFramePollError::FrameLengthExceedsBuffer {
            requested: 4,
            available: 3,
            report,
        } if report.fuel_consumed() > 0
    ));
}

fn state(bytes: &[u8]) -> SessionState {
    let mut state = SessionState::new(nonzero(16));
    state
        .update(|_| {
            Ok::<_, Infallible>(SessionStateAction::Replace(
                bytes.to_vec().into_boxed_slice(),
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

fn nonzero(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("test value is nonzero")
}
