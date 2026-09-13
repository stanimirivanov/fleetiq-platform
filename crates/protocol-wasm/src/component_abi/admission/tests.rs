use std::{num::NonZeroUsize, time::Duration};

use wit_component::{ComponentEncoder, StringEncoding, dummy_module, embed_component_metadata};
use wit_parser::{ManglingAndAbi, Resolve};

use super::{ComponentAdmissionError, ComponentAdmissionPolicy};
use crate::{PROTOCOL_PACKAGE_WIT, WasmSandbox};

const IMPORTING_WIT: &str = r"
    package test:imports@0.1.0;

    interface host {
        tick: func();
    }

    world importing-package {
        import host;
    }
";

const INCOMPATIBLE_WIT: &str = r"
    package fleetiq:protocol-package@0.1.0;

    interface runtime {
        locate-frame: func() -> u32;
    }

    world protocol-package {
        export runtime;
    }
";

#[test]
fn exact_protocol_world_is_compiled_and_bound_during_admission() {
    let component = component_from_wit(PROTOCOL_PACKAGE_WIT, "protocol-package");
    let sandbox = sandbox();

    let compiled = sandbox
        .compile_protocol_component(&component, policy(component.len()))
        .expect("the generated fixture implements the exact protocol world");

    assert_eq!(format!("{compiled:?}"), "CompiledProtocolComponent { .. }");
}

#[test]
fn component_at_the_raw_byte_limit_is_accepted() {
    let component = component_from_wit(PROTOCOL_PACKAGE_WIT, "protocol-package");

    assert!(
        sandbox()
            .compile_protocol_component(&component, policy(component.len()))
            .is_ok()
    );
}

#[test]
fn oversized_component_is_rejected_before_compilation() {
    let component = component_from_wit(PROTOCOL_PACKAGE_WIT, "protocol-package");
    let limit = component.len() - 1;

    let error = sandbox()
        .compile_protocol_component(&component, policy(limit))
        .unwrap_err();

    assert!(matches!(
        error,
        ComponentAdmissionError::ComponentTooLarge { limit: actual_limit, actual }
            if actual_limit == limit && actual == component.len()
    ));
}

#[test]
fn malformed_component_reports_compilation_failure() {
    let error = sandbox()
        .compile_protocol_component(&[0x00, 0x61], policy(2))
        .unwrap_err();

    assert!(matches!(error, ComponentAdmissionError::Compilation(_)));
}

#[test]
fn component_requesting_host_imports_is_rejected() {
    let component = component_from_wit(IMPORTING_WIT, "importing-package");

    let error = sandbox()
        .compile_protocol_component(&component, policy(component.len()))
        .unwrap_err();

    assert!(matches!(
        error,
        ComponentAdmissionError::HostImportsNotAllowed { count: 1 }
    ));
}

#[test]
fn incompatible_protocol_interface_is_rejected_during_admission() {
    let component = component_from_wit(INCOMPATIBLE_WIT, "protocol-package");

    let error = sandbox()
        .compile_protocol_component(&component, policy(component.len()))
        .unwrap_err();

    assert!(matches!(
        error,
        ComponentAdmissionError::InterfaceMismatch(_)
    ));
}

fn sandbox() -> WasmSandbox {
    WasmSandbox::new(Duration::from_millis(2), nonzero(1 << 20))
        .expect("test sandbox configuration is valid")
}

fn policy(max_component_bytes: usize) -> ComponentAdmissionPolicy {
    ComponentAdmissionPolicy::new(nonzero(max_component_bytes))
}

fn nonzero(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("test value is nonzero")
}

fn component_from_wit(source: &str, world_name: &str) -> Vec<u8> {
    let mut resolve = Resolve::default();
    let package = resolve
        .push_str("fixture.wit", source)
        .expect("test WIT is valid");
    let world = resolve
        .select_world(&[package], Some(world_name))
        .expect("test world exists");
    let mut module = dummy_module(&resolve, world, ManglingAndAbi::Standard32);
    embed_component_metadata(&mut module, &resolve, world, StringEncoding::UTF8)
        .expect("component metadata can be embedded");
    ComponentEncoder::default()
        .module(&module)
        .expect("dummy module is valid")
        .validate(true)
        .encode()
        .expect("dummy module can be componentized")
}
