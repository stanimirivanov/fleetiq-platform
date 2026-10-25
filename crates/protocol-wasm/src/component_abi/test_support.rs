//! Executable protocol-component fixtures shared by adapter tests.

use std::{
    num::{NonZeroU64, NonZeroUsize},
    time::Duration,
};

use wit_component::{ComponentEncoder, StringEncoding, embed_component_metadata};
use wit_parser::Resolve;

use crate::{
    AdmittedProtocolComponent, ComponentAdmissionPolicy, InvocationBudget, PROTOCOL_PACKAGE_WIT,
    WasmSandbox,
};

#[derive(Clone, Copy)]
pub(super) enum Fixture {
    StatefulFrame,
    NeedMoreData,
    StateLengthFrame,
    StateLengthFrameAndReplaceState,
    OversizedFrame,
    FrameThenReject,
    PackageRejection,
    ZeroLengthFrame,
    ReplaceState,
    Infinite,
    Trap,
}

pub(super) fn compiled(fixture: Fixture) -> (WasmSandbox, AdmittedProtocolComponent) {
    let bytes = component(fixture);
    let sandbox = sandbox();
    let policy = ComponentAdmissionPolicy::new(nonzero_usize(bytes.len()));
    let component = sandbox
        .admit_protocol_component(package(), &bytes, policy)
        .expect("fixture implements the protocol-package world");
    (sandbox, component)
}

fn package() -> fleetiq_telemetry::ProtocolPackageRef {
    fleetiq_telemetry::ProtocolPackageRef::new(
        fleetiq_telemetry::ProtocolPackageId::new("fixture.protocol").unwrap(),
        fleetiq_telemetry::ProtocolPackageVersion::new("1").unwrap(),
    )
}

pub(super) fn budget() -> InvocationBudget {
    InvocationBudget::new(
        NonZeroU64::new(1_000_000).unwrap(),
        Duration::from_secs(1),
        nonzero_usize(1 << 20),
        nonzero_usize(1_000),
    )
    .unwrap()
}

fn component(fixture: Fixture) -> Vec<u8> {
    let mut module = core_module(fixture);
    let mut resolve = Resolve::default();
    let package = resolve
        .push_str("fixture.wit", PROTOCOL_PACKAGE_WIT)
        .expect("protocol-package WIT is valid");
    let world = resolve
        .select_world(&[package], Some("protocol-package"))
        .expect("protocol-package world exists");
    // Preserve the fixtures' existing WIT names; canonical naming is opt-in.
    embed_component_metadata(&mut module, &resolve, world, StringEncoding::UTF8, false)
        .expect("component metadata can be embedded");
    ComponentEncoder::default()
        .module(&module)
        .expect("fixture module is valid")
        .validate(true)
        .encode()
        .expect("fixture module can be componentized")
}

fn core_module(fixture: Fixture) -> Vec<u8> {
    let locate_body = locate_body(fixture);
    let process_body = process_body(fixture);

    wat::parse_str(format!(
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
            {process_body}
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
    .expect("fixture core module is valid")
}

fn locate_body(fixture: Fixture) -> &'static str {
    match fixture {
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
        Fixture::NeedMoreData | Fixture::ReplaceState => "i32.const 0",
        Fixture::StateLengthFrame | Fixture::StateLengthFrameAndReplaceState => {
            FRAME_FROM_STATE_LENGTH
        }
        Fixture::OversizedFrame | Fixture::FrameThenReject => FOUR_BYTE_FRAME,
        Fixture::PackageRejection => package_rejection_body(),
        Fixture::ZeroLengthFrame => ZERO_BYTE_FRAME,
        Fixture::Infinite => "(loop $spin br $spin) unreachable",
        Fixture::Trap => "unreachable",
    }
}

fn process_body(fixture: Fixture) -> &'static str {
    match fixture {
        Fixture::PackageRejection | Fixture::FrameThenReject => package_rejection_body(),
        Fixture::ReplaceState | Fixture::StateLengthFrameAndReplaceState => {
            r"
            i32.const 12
            i32.const 1
            i32.store
            i32.const 16
            i32.const 64
            i32.store
            i32.const 20
            i32.const 4
            i32.store
            i32.const 64
            i32.const 0x7478656e
            i32.store
            i32.const 0
        "
        }
        Fixture::Trap => "unreachable",
        _ => "i32.const 0",
    }
}

const FRAME_FROM_STATE_LENGTH: &str = r"
    i32.const 0
    i32.const 0
    i32.store
    i32.const 4
    i32.const 1
    i32.store
    i32.const 8
    local.get 3
    i32.store
    i32.const 0
";

const FOUR_BYTE_FRAME: &str = r"
    i32.const 0
    i32.const 0
    i32.store
    i32.const 4
    i32.const 1
    i32.store
    i32.const 8
    i32.const 4
    i32.store
    i32.const 0
";

const ZERO_BYTE_FRAME: &str = r"
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
";

const fn package_rejection_body() -> &'static str {
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

pub(super) fn sandbox() -> WasmSandbox {
    WasmSandbox::new(Duration::from_millis(2), nonzero_usize(1 << 20))
        .expect("test sandbox configuration is valid")
}

fn nonzero_usize(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("test value is nonzero")
}
