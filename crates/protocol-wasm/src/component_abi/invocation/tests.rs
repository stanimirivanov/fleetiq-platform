use std::num::NonZeroUsize;

use fleetiq_protocol_sdk::{FrameBoundary, FrameProcessingInput, FramingInput, SessionStateAction};

use super::ProtocolComponentInvocationError;
use crate::{
    ComponentOutputError, InvocationError,
    component_abi::test_support::{Fixture, budget, compiled},
};
#[test]
fn framing_returns_a_validated_value_and_resource_report() {
    let (sandbox, component) = compiled(Fixture::StatefulFrame);

    let result = sandbox
        .invoke_protocol_framing(
            &component,
            budget(),
            FramingInput::new(b"unread", b"session"),
        )
        .unwrap();

    assert_eq!(
        result.value(),
        &FrameBoundary::Frame {
            length: NonZeroUsize::new(1).unwrap(),
        }
    );
    assert!(result.report().fuel_consumed() > 0);
}

#[test]
fn every_invocation_uses_fresh_guest_state() {
    let (sandbox, component) = compiled(Fixture::StatefulFrame);

    let first = sandbox
        .invoke_protocol_framing(&component, budget(), FramingInput::new(&[], &[]))
        .unwrap();
    let second = sandbox
        .invoke_protocol_framing(&component, budget(), FramingInput::new(&[], &[]))
        .unwrap();

    assert_eq!(first.value(), second.value());
    assert_eq!(
        second.value(),
        &FrameBoundary::Frame {
            length: NonZeroUsize::new(1).unwrap(),
        }
    );
}

#[test]
fn processing_lifts_an_empty_atomic_proposal() {
    let (sandbox, component) = compiled(Fixture::StatefulFrame);

    let result = sandbox
        .invoke_protocol_processing(
            &component,
            budget(),
            FrameProcessingInput::new(b"frame", b"session"),
        )
        .unwrap();
    let (output, report) = result.into_parts();

    assert!(output.decoded_outputs().is_empty());
    assert_eq!(output.session_state_action(), &SessionStateAction::Keep);
    assert!(output.replies().is_empty());
    assert!(report.fuel_consumed() > 0);
}

#[test]
fn package_rejection_remains_distinct_from_runtime_failure() {
    let (sandbox, component) = compiled(Fixture::PackageRejection);

    let error = sandbox
        .invoke_protocol_framing(&component, budget(), FramingInput::new(&[], &[]))
        .unwrap_err();

    assert!(matches!(
        error,
        ProtocolComponentInvocationError::PackageRejected { code: 42, report }
            if report.fuel_consumed() > 0
    ));
}

#[test]
fn invalid_lifted_output_reports_semantic_failure() {
    let (sandbox, component) = compiled(Fixture::ZeroLengthFrame);

    let error = sandbox
        .invoke_protocol_framing(&component, budget(), FramingInput::new(&[], &[]))
        .unwrap_err();

    assert!(matches!(
        error,
        ProtocolComponentInvocationError::InvalidOutput {
            source: ComponentOutputError::ZeroFrameLength,
            report,
        } if report.fuel_consumed() > 0
    ));
}

#[test]
fn component_execution_is_stopped_by_the_shared_fuel_budget() {
    let (sandbox, component) = compiled(Fixture::Infinite);

    let error = sandbox
        .invoke_protocol_framing(&component, budget(), FramingInput::new(&[], &[]))
        .unwrap_err();

    assert!(matches!(
        error,
        ProtocolComponentInvocationError::Invocation(InvocationError::FuelExhausted)
    ));
}

#[test]
fn trapping_component_uses_shared_invocation_classification() {
    let (sandbox, component) = compiled(Fixture::Trap);

    let error = sandbox
        .invoke_protocol_framing(&component, budget(), FramingInput::new(&[], &[]))
        .unwrap_err();

    assert!(matches!(
        error,
        ProtocolComponentInvocationError::Invocation(InvocationError::Runtime(_))
    ));
}
