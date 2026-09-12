use std::{num::NonZeroU32, time::Duration};

use super::{GuestMemoryRegion, OpaqueTransferError, OpaqueTransferLimits, TransferExportNames};
use crate::{
    CompiledTransferModule, InvocationBudget, InvocationError, ModuleAdmissionError,
    ModuleAdmissionPolicy, TransferExportRole, WasmSandbox,
};

const ECHO_MODULE: &str = r#"
    (module
        (memory (export "memory") 1)
        (func (export "allocate") (param i32) (result i32)
            i32.const 32)
        (func (export "process") (param $pointer i32) (param $length i32) (result i64)
            local.get $pointer
            i64.extend_i32_u
            i64.const 32
            i64.shl
            local.get $length
            i64.extend_i32_u
            i64.or))
"#;

fn nonzero_u32(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).expect("test transfer limit is nonzero")
}

fn sandbox() -> WasmSandbox {
    WasmSandbox::new(
        Duration::from_millis(5),
        std::num::NonZeroUsize::new(64 * 1024).expect("test stack limit is nonzero"),
    )
    .expect("sandbox starts")
}

fn budget(memory_bytes: usize) -> InvocationBudget {
    InvocationBudget::new(
        std::num::NonZeroU64::new(100_000).expect("test fuel is nonzero"),
        Duration::from_secs(1),
        std::num::NonZeroUsize::new(memory_bytes).expect("test memory limit is nonzero"),
        std::num::NonZeroUsize::new(1).expect("test table limit is nonzero"),
    )
    .expect("test budget is valid")
}

fn limits(input: u32, output: u32) -> OpaqueTransferLimits {
    OpaqueTransferLimits::new(nonzero_u32(input), nonzero_u32(output))
}

fn exports() -> TransferExportNames<'static> {
    TransferExportNames::new("memory", "allocate", "process")
}

fn compile(sandbox: &WasmSandbox, source: &str) -> CompiledTransferModule {
    let wasm = wat::parse_str(source).expect("test WebAssembly text is valid");
    sandbox
        .compile_transfer(
            &wasm,
            ModuleAdmissionPolicy::new(
                std::num::NonZeroUsize::new(wasm.len()).expect("test module is nonempty"),
            ),
            exports(),
        )
        .expect("test transfer module is admitted")
}

#[test]
fn transfer_export_names_and_limits_preserve_explicit_configuration() {
    let names = exports();
    let limits = limits(128, 256);

    assert_eq!(names.memory(), "memory");
    assert_eq!(names.allocate(), "allocate");
    assert_eq!(names.process(), "process");
    assert_eq!(limits.max_input_bytes().get(), 128);
    assert_eq!(limits.max_output_bytes().get(), 256);
}

#[test]
fn admission_requires_the_selected_memory_export() {
    let sandbox = sandbox();
    let source = ECHO_MODULE.replace("\"memory\"", "\"other-memory\"");
    let wasm = wat::parse_str(source).expect("test WebAssembly text is valid");

    assert!(matches!(
        sandbox.compile_transfer(
            &wasm,
            ModuleAdmissionPolicy::new(
                std::num::NonZeroUsize::new(wasm.len()).expect("module is nonempty")
            ),
            exports(),
        ),
        Err(ModuleAdmissionError::TransferExportMissing {
            role: TransferExportRole::Memory,
            export,
        }) if export.as_ref() == "memory"
    ));
}

#[test]
fn admission_rejects_an_export_with_the_wrong_external_kind() {
    let sandbox = sandbox();
    let source = r#"
        (module
            (memory 1)
            (func (export "memory"))
            (func (export "allocate") (param i32) (result i32) i32.const 0)
            (func (export "process") (param i32 i32) (result i64) i64.const 0))
    "#;
    let wasm = wat::parse_str(source).expect("test WebAssembly text is valid");

    assert!(matches!(
        sandbox.compile_transfer(
            &wasm,
            ModuleAdmissionPolicy::new(
                std::num::NonZeroUsize::new(wasm.len()).expect("module is nonempty")
            ),
            exports(),
        ),
        Err(ModuleAdmissionError::TransferExportKindMismatch {
            role: TransferExportRole::Memory,
            export,
        }) if export.as_ref() == "memory"
    ));
}

#[test]
fn admission_rejects_an_incompatible_transfer_function_signature() {
    let sandbox = sandbox();
    let source = ECHO_MODULE.replace(
        "(func (export \"allocate\") (param i32) (result i32)",
        "(func (export \"allocate\") (param i64) (result i32)",
    );
    let wasm = wat::parse_str(source).expect("test WebAssembly text is valid");

    assert!(matches!(
        sandbox.compile_transfer(
            &wasm,
            ModuleAdmissionPolicy::new(
                std::num::NonZeroUsize::new(wasm.len()).expect("module is nonempty")
            ),
            exports(),
        ),
        Err(ModuleAdmissionError::TransferExportSignatureMismatch {
            role: TransferExportRole::Allocate,
            export,
            actual,
        }) if export.as_ref() == "allocate" && actual.contains("i64")
    ));
}

#[test]
fn opaque_bytes_round_trip_through_validated_guest_memory() {
    let sandbox = sandbox();
    let module = compile(&sandbox, ECHO_MODULE);

    let output = sandbox
        .invoke_transfer(&module, budget(65_536), limits(16, 16), b"secret")
        .expect("bounded transfer succeeds");

    assert_eq!(output.as_bytes(), b"secret");
    assert!(output.report().fuel_consumed() > 0);
    let debug = format!("{output:?}");
    assert!(debug.contains("length: 6"));
    assert!(!debug.contains("secret"));
}

#[test]
fn oversized_host_input_is_rejected_before_guest_execution() {
    let sandbox = sandbox();
    let module = compile(&sandbox, ECHO_MODULE);

    assert!(matches!(
        sandbox.invoke_transfer(&module, budget(65_536), limits(3, 8), b"four"),
        Err(OpaqueTransferError::InputTooLarge {
            limit: 3,
            actual: 4,
        })
    ));
}

#[test]
fn allocator_range_is_validated_before_host_writes() {
    let sandbox = sandbox();
    let module = compile(
        &sandbox,
        &ECHO_MODULE.replace("i32.const 32", "i32.const 65535"),
    );

    assert!(matches!(
        sandbox.invoke_transfer(&module, budget(65_536), limits(8, 8), b"two"),
        Err(OpaqueTransferError::GuestMemoryOutOfBounds {
            region: GuestMemoryRegion::Input,
            offset: 65_535,
            length: 3,
            memory_size: 65_536,
        })
    ));
}

#[test]
fn output_limit_is_checked_before_host_allocation_or_read() {
    let sandbox = sandbox();
    let module = compile(
        &sandbox,
        r#"
        (module
            (memory (export "memory") 1)
            (func (export "allocate") (param i32) (result i32) i32.const 0)
            (func (export "process") (param i32 i32) (result i64)
                i64.const 5))
        "#,
    );

    assert!(matches!(
        sandbox.invoke_transfer(&module, budget(65_536), limits(8, 4), b"in"),
        Err(OpaqueTransferError::OutputTooLarge {
            limit: 4,
            actual: 5,
        })
    ));
}

#[test]
fn processor_range_is_validated_before_host_reads() {
    let sandbox = sandbox();
    let module = compile(
        &sandbox,
        r#"
        (module
            (memory (export "memory") 1)
            (func (export "allocate") (param i32) (result i32) i32.const 0)
            (func (export "process") (param i32 i32) (result i64)
                i64.const 281470681743362))
        "#,
    );

    assert!(matches!(
        sandbox.invoke_transfer(&module, budget(65_536), limits(8, 8), b"in"),
        Err(OpaqueTransferError::GuestMemoryOutOfBounds {
            region: GuestMemoryRegion::Output,
            offset: 65_535,
            length: 2,
            memory_size: 65_536,
        })
    ));
}

#[test]
fn zero_length_regions_may_point_to_the_end_of_memory() {
    let sandbox = sandbox();
    let module = compile(
        &sandbox,
        r#"
        (module
            (memory (export "memory") 1)
            (func (export "allocate") (param i32) (result i32) i32.const 65536)
            (func (export "process") (param i32 i32) (result i64)
                i64.const 281474976710656))
        "#,
    );

    let output = sandbox
        .invoke_transfer(&module, budget(65_536), limits(1, 1), b"")
        .expect("an empty region at memory end is valid");

    assert!(output.as_bytes().is_empty());
}

#[test]
fn allocator_growth_uses_the_existing_linear_memory_limit() {
    let sandbox = sandbox();
    let module = compile(
        &sandbox,
        r#"
        (module
            (memory (export "memory") 1 2)
            (func (export "allocate") (param i32) (result i32)
                i32.const 1
                memory.grow
                drop
                i32.const 0)
            (func (export "process") (param i32 i32) (result i64) i64.const 0))
        "#,
    );

    assert!(matches!(
        sandbox.invoke_transfer(&module, budget(65_536), limits(8, 8), b"in"),
        Err(OpaqueTransferError::Invocation(
            InvocationError::LinearMemoryLimitExceeded {
                limit: 65_536,
                attempted: 131_072,
            }
        ))
    ));
}

#[test]
fn processor_fuel_exhaustion_uses_shared_invocation_classification() {
    let sandbox = sandbox();
    let module = compile(
        &sandbox,
        r#"
        (module
            (memory (export "memory") 1)
            (func (export "allocate") (param i32) (result i32) i32.const 0)
            (func (export "process") (param i32 i32) (result i64)
                (loop $forever br $forever)
                unreachable))
        "#,
    );

    assert!(matches!(
        sandbox.invoke_transfer(&module, budget(65_536), limits(8, 8), b"in"),
        Err(OpaqueTransferError::Invocation(
            InvocationError::FuelExhausted
        ))
    ));
}

#[test]
fn each_transfer_uses_a_fresh_instance_for_guest_allocation_cleanup() {
    let sandbox = sandbox();
    let module = compile(
        &sandbox,
        r#"
        (module
            (memory (export "memory") 1)
            (global $next (mut i32) (i32.const 65530))
            (func (export "allocate") (param $length i32) (result i32)
                (local $allocated i32)
                global.get $next
                local.tee $allocated
                local.get $length
                i32.add
                global.set $next
                local.get $allocated)
            (func (export "process") (param $pointer i32) (param $length i32) (result i64)
                local.get $pointer
                i64.extend_i32_u
                i64.const 32
                i64.shl
                local.get $length
                i64.extend_i32_u
                i64.or))
        "#,
    );

    let first = sandbox
        .invoke_transfer(&module, budget(65_536), limits(8, 8), b"first")
        .expect("first transfer succeeds");
    let second = sandbox
        .invoke_transfer(&module, budget(65_536), limits(8, 8), b"next")
        .expect("second transfer succeeds in a fresh instance");

    assert_eq!(first.as_bytes(), b"first");
    assert_eq!(second.as_bytes(), b"next");
}
