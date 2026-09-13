use std::{
    num::{NonZeroU64, NonZeroUsize},
    time::Duration,
};

use fleetiq_protocol_sdk::{FrameBoundary, FrameProcessingInput, FramingInput, SessionStateAction};
use wit_component::{ComponentEncoder, StringEncoding, embed_component_metadata};
use wit_parser::Resolve;

use super::ProtocolComponentInvocationError;
use crate::{
    ComponentAdmissionPolicy, ComponentOutputError, InvocationBudget, InvocationError,
    PROTOCOL_PACKAGE_WIT, WasmSandbox,
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

#[derive(Clone, Copy)]
enum Fixture {
    StatefulFrame,
    PackageRejection,
    ZeroLengthFrame,
    Infinite,
    Trap,
}

fn compiled(fixture: Fixture) -> (WasmSandbox, crate::CompiledProtocolComponent) {
    let bytes = component(fixture);
    let sandbox = sandbox();
    let policy = ComponentAdmissionPolicy::new(nonzero_usize(bytes.len()));
    let component = sandbox
        .compile_protocol_component(&bytes, policy)
        .expect("fixture implements the protocol-package world");
    (sandbox, component)
}

fn component(fixture: Fixture) -> Vec<u8> {
    let locate_body = match fixture {
        Fixture::StatefulFrame => {
            r"
            i32.const 0
            i32.const 0
            i32.store
            i32.const 4
            i32.const 1
            i32.store
            global.get $calls
            i32.const 1
            i32.add
            global.set $calls
            i32.const 8
            global.get $calls
            i32.store
            i32.const 0
        "
        }
        Fixture::PackageRejection => {
            r"
            i32.const 0
            i32.const 1
            i32.store
            i32.const 4
            i32.const 42
            i32.store
            i32.const 0
        "
        }
        Fixture::ZeroLengthFrame => {
            r"
            i32.const 0
            i32.const 0
            i32.store
            i32.const 4
            i32.const 1
            i32.store
            i32.const 8
            i32.const 0
            i32.store
            i32.const 0
        "
        }
        Fixture::Infinite => "(loop $spin br $spin) unreachable",
        Fixture::Trap => "unreachable",
    };
    let module = wat::parse_str(format!(
        r#"
        (module
          (type $call (func (param i32 i32 i32 i32) (result i32)))
          (type $post (func (param i32)))
          (type $initialize (func))
          (memory (export "cm32p2_memory") 1)
          (global $heap (mut i32) (i32.const 1024))
          (global $calls (mut i32) (i32.const 0))
          (func (export "cm32p2|fleetiq:protocol-package/runtime@0.1|locate-frame")
                (type $call) (param i32 i32 i32 i32) (result i32)
            {locate_body}
          )
          (func (export "cm32p2|fleetiq:protocol-package/runtime@0.1|locate-frame_post")
                (type $post) (param i32))
          (func (export "cm32p2|fleetiq:protocol-package/runtime@0.1|process-frame")
                (type $call) (param i32 i32 i32 i32) (result i32)
            i32.const 0
          )
          (func (export "cm32p2|fleetiq:protocol-package/runtime@0.1|process-frame_post")
                (type $post) (param i32))
          (func (export "cm32p2_realloc")
                (type $call) (param i32 i32 i32 i32) (result i32)
                (local $result i32)
            global.get $heap
            local.tee $result
            local.get 3
            i32.add
            global.set $heap
            local.get $result
          )
          (func (export "cm32p2_initialize") (type $initialize))
        )
        "#
    ))
    .expect("fixture core module is valid");

    let mut resolve = Resolve::default();
    let package = resolve
        .push_str("fixture.wit", PROTOCOL_PACKAGE_WIT)
        .expect("protocol-package WIT is valid");
    let world = resolve
        .select_world(&[package], Some("protocol-package"))
        .expect("protocol-package world exists");
    let mut module = module;
    embed_component_metadata(&mut module, &resolve, world, StringEncoding::UTF8)
        .expect("component metadata can be embedded");
    ComponentEncoder::default()
        .module(&module)
        .expect("fixture module is valid")
        .validate(true)
        .encode()
        .expect("fixture module can be componentized")
}

fn sandbox() -> WasmSandbox {
    WasmSandbox::new(Duration::from_millis(2), nonzero_usize(1 << 20))
        .expect("test sandbox configuration is valid")
}

fn budget() -> InvocationBudget {
    InvocationBudget::new(
        NonZeroU64::new(1_000_000).unwrap(),
        Duration::from_secs(1),
        nonzero_usize(1 << 20),
        nonzero_usize(1_000),
    )
    .unwrap()
}

fn nonzero_usize(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("test value is nonzero")
}
