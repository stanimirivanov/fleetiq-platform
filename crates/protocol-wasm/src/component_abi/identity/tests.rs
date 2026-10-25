use super::*;
use crate::{PROTOCOL_PACKAGE_WIT, component_abi::test_support};
use fleetiq_protocol_sdk::FramingInput;
use std::{
    fmt::Write as _,
    num::{NonZeroU64, NonZeroUsize},
    time::Duration,
};
use wit_component::{ComponentEncoder, StringEncoding, dummy_module, embed_component_metadata};
use wit_parser::{ManglingAndAbi, Resolve};
fn component(source: &str, world: &str, empty_claim: bool) -> Vec<u8> {
    let mut resolve = Resolve::default();
    let package = resolve.push_str("test.wit", source).unwrap();
    let world = resolve.select_world(&[package], Some(world)).unwrap();
    let mut module = if empty_claim {
        let mut wat = String::from(
            "(module (memory (export \"cm32p2_memory\") 1) (global $heap (mut i32) (i32.const 1024))",
        );
        for name in [
            "runtime@0.1|locate-frame",
            "runtime@0.1|process-frame",
            "identification@0.1|identify",
        ] {
            write!(wat, "(func (export \"cm32p2|fleetiq:protocol-package/{name}\") (param i32 i32 i32 i32) (result i32) i32.const 0) (func (export \"cm32p2|fleetiq:protocol-package/{name}_post\") (param i32))").unwrap();
        }
        wat.push_str("(func (export \"cm32p2_realloc\") (param i32 i32 i32 i32) (result i32) (local i32) global.get $heap local.tee 4 local.get 3 i32.add global.set $heap local.get 4) (func (export \"cm32p2_initialize\")))");
        wat::parse_str(wat).unwrap()
    } else {
        dummy_module(&resolve, world, ManglingAndAbi::Standard32)
    };
    embed_component_metadata(&mut module, &resolve, world, StringEncoding::UTF8, false).unwrap();
    ComponentEncoder::default()
        .module(&module)
        .unwrap()
        .validate(true)
        .encode()
        .unwrap()
}
fn package() -> ProtocolPackageRef {
    ProtocolPackageRef::new(
        fleetiq_telemetry::ProtocolPackageId::new("test.identity").unwrap(),
        fleetiq_telemetry::ProtocolPackageVersion::new("1").unwrap(),
    )
}
fn nz(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).unwrap()
}
#[test]
fn legacy_world_stays_usable_but_does_not_gain_an_identity_export() {
    let bytes = component(PROTOCOL_PACKAGE_WIT, "protocol-package", false);
    let sandbox = test_support::sandbox();
    let policy = ComponentAdmissionPolicy::new(nz(bytes.len()));
    assert!(
        sandbox
            .admit_protocol_component(package(), &bytes, policy)
            .is_ok()
    );
    assert!(matches!(
        sandbox.admit_identity_protocol_component(package(), &bytes, policy),
        Err(ComponentAdmissionError::InterfaceMismatch(_))
    ));
}
#[test]
fn identity_world_cannot_import_host_capabilities() {
    let source = format!(
        "{PROTOCOL_PACKAGE_WIT}\n{IDENTITY_PROTOCOL_PACKAGE_WIT}\nworld imported {{ include identity-protocol-package; import dangerous: func(); }}"
    );
    let bytes = component(&source, "imported", false);
    let sandbox = test_support::sandbox();
    assert!(matches!(
        sandbox.admit_identity_protocol_component(
            package(),
            &bytes,
            ComponentAdmissionPolicy::new(nz(bytes.len()))
        ),
        Err(ComponentAdmissionError::HostImportsNotAllowed { .. })
    ));
}
#[test]
fn invalid_lifted_claim_is_rejected_by_host_validation() {
    let source = format!("{PROTOCOL_PACKAGE_WIT}\n{IDENTITY_PROTOCOL_PACKAGE_WIT}");
    let bytes = component(&source, "identity-protocol-package", true);
    let sandbox = test_support::sandbox();
    let admitted = sandbox
        .admit_identity_protocol_component(
            package(),
            &bytes,
            ComponentAdmissionPolicy::new(nz(bytes.len())),
        )
        .unwrap();
    let budget = InvocationBudget::new(
        NonZeroU64::new(1_000_000).unwrap(),
        Duration::from_secs(2),
        nz(1 << 20),
        nz(1000),
    )
    .unwrap();
    assert!(matches!(
        sandbox.identify_component_device(&admitted, budget, FramingInput::new(b"x", b""), nz(16)),
        Err(IdentityInvocationError::InvalidClaim)
    ));
}
