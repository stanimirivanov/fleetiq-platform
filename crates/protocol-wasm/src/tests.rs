use std::{
    num::{NonZeroU64, NonZeroUsize},
    time::Duration,
};

use wasmtime::Trap;

use super::{
    CompiledModule, InvocationBudget, InvocationBudgetError, InvocationError, ModuleAdmissionError,
    ModuleAdmissionPolicy, SandboxCreationError, WasmSandbox,
};

fn fuel(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).expect("test fuel is nonzero")
}

fn nonzero_usize(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("test size is nonzero")
}

fn sandbox() -> WasmSandbox {
    WasmSandbox::new(Duration::from_millis(5), nonzero_usize(64 * 1024)).expect("sandbox starts")
}

fn budget(
    fuel_value: u64,
    wall_time: Duration,
    memory_bytes: usize,
    table_elements: usize,
) -> InvocationBudget {
    InvocationBudget::new(
        fuel(fuel_value),
        wall_time,
        nonzero_usize(memory_bytes),
        nonzero_usize(table_elements),
    )
    .expect("test budget is valid")
}

fn compile(sandbox: &WasmSandbox, source: &str) -> CompiledModule {
    let wasm = wat::parse_str(source).expect("test WebAssembly text is valid");
    sandbox
        .compile(&wasm, module_policy(wasm.len()))
        .expect("test module compiles")
}

fn module_policy(max_module_bytes: usize) -> ModuleAdmissionPolicy {
    ModuleAdmissionPolicy::new(nonzero_usize(max_module_bytes))
}

#[test]
fn rejects_zero_epoch_tick_and_wall_time() {
    assert!(matches!(
        WasmSandbox::new(Duration::ZERO, nonzero_usize(64 * 1024)),
        Err(SandboxCreationError::ZeroEpochTick)
    ));
    assert_eq!(
        InvocationBudget::new(
            fuel(1),
            Duration::ZERO,
            nonzero_usize(65_536),
            nonzero_usize(1),
        ),
        Err(InvocationBudgetError::ZeroWallTime)
    );
}

#[test]
fn module_larger_than_the_admission_limit_is_rejected_before_compilation() {
    let sandbox = sandbox();
    let oversized_malformed_bytes = b"not WebAssembly";
    let limit = oversized_malformed_bytes.len() - 1;

    assert!(matches!(
        sandbox.compile(oversized_malformed_bytes, module_policy(limit)),
        Err(ModuleAdmissionError::ModuleTooLarge { limit: error_limit, actual })
            if error_limit == limit && actual == oversized_malformed_bytes.len()
    ));
}

#[test]
fn module_at_the_admission_limit_is_compiled() {
    let sandbox = sandbox();
    let wasm = wat::parse_str("(module)").expect("test WebAssembly text is valid");
    let policy = module_policy(wasm.len());

    sandbox
        .compile(&wasm, policy)
        .expect("a module exactly at the byte limit is admitted");
    assert_eq!(policy.max_module_bytes().get(), wasm.len());
}

#[test]
fn malformed_module_reports_the_compilation_failure() {
    let sandbox = sandbox();

    assert!(matches!(
        sandbox.compile(b"not WebAssembly", module_policy(64)),
        Err(ModuleAdmissionError::Compilation(_))
    ));
}

#[test]
fn module_requesting_host_imports_is_rejected_during_admission() {
    let sandbox = sandbox();
    let wasm = wat::parse_str(r#"(module (import "host" "clock" (func)))"#)
        .expect("test WebAssembly text is valid");

    assert!(matches!(
        sandbox.compile(&wasm, module_policy(wasm.len())),
        Err(ModuleAdmissionError::HostImportsNotAllowed { count: 1 })
    ));
}

#[test]
fn module_with_multiple_tables_is_rejected_during_admission() {
    let sandbox = sandbox();
    let wasm = wat::parse_str(
        r#"
        (module
            (table 1 funcref)
            (table 1 funcref)
            (func (export "run")))
        "#,
    )
    .expect("test WebAssembly text is valid");

    assert!(matches!(
        sandbox.compile(&wasm, module_policy(wasm.len())),
        Err(ModuleAdmissionError::TooManyTables {
            limit: 1,
            actual: 2,
        })
    ));
}

#[test]
fn finite_guest_completes_and_reports_consumed_fuel() {
    let sandbox = sandbox();
    let module = compile(&sandbox, r#"(module (func (export "run") nop))"#);
    let budget = budget(10_000, Duration::from_secs(1), 65_536, 1);

    let report = sandbox
        .invoke_probe(&module, "run", budget)
        .expect("finite guest completes");

    assert!(report.fuel_consumed() > 0);
    assert!(report.fuel_consumed() <= budget.fuel().get());
}

#[test]
fn deterministic_fuel_stops_an_infinite_guest() {
    let sandbox = sandbox();
    let module = compile(
        &sandbox,
        r#"(module (func (export "run") (loop $forever br $forever)))"#,
    );
    let budget = budget(100, Duration::from_secs(5), 65_536, 1);

    assert!(matches!(
        sandbox.invoke_probe(&module, "run", budget),
        Err(InvocationError::FuelExhausted)
    ));
}

#[test]
fn epoch_deadline_stops_an_infinite_guest_with_abundant_fuel() {
    let sandbox = sandbox();
    let module = compile(
        &sandbox,
        r#"(module (func (export "run") (loop $forever br $forever)))"#,
    );
    let budget = budget(u64::MAX, Duration::from_millis(20), 65_536, 1);

    assert!(matches!(
        sandbox.invoke_probe(&module, "run", budget),
        Err(InvocationError::DeadlineExceeded)
    ));
}

#[test]
fn recursive_guest_hits_the_configured_stack_limit() {
    let sandbox = sandbox();
    let module = compile(
        &sandbox,
        r#"
        (module
            (func $recurse (export "run")
                call $recurse))
        "#,
    );
    let budget = budget(u64::MAX, Duration::from_secs(5), 65_536, 1);

    assert_eq!(sandbox.max_wasm_stack_bytes().get(), 64 * 1024);
    assert!(matches!(
        sandbox.invoke_probe(&module, "run", budget),
        Err(InvocationError::StackLimitExceeded {
            configured_limit: 65_536,
        })
    ));
}

#[test]
fn ordinary_guest_traps_remain_distinct_from_resource_exhaustion() {
    let sandbox = sandbox();
    let module = compile(&sandbox, r#"(module (func (export "run") unreachable))"#);
    let budget = budget(10_000, Duration::from_secs(1), 65_536, 1);

    let Err(InvocationError::Runtime(error)) = sandbox.invoke_probe(&module, "run", budget) else {
        panic!("an ordinary guest trap should remain a runtime failure");
    };

    assert_eq!(
        error.downcast_ref::<Trap>(),
        Some(&Trap::UnreachableCodeReached)
    );
}

#[test]
fn initial_linear_memory_cannot_exceed_the_invocation_limit() {
    let sandbox = sandbox();
    let module = compile(&sandbox, r#"(module (memory 2) (func (export "run")))"#);

    assert!(matches!(
        sandbox.invoke_probe(
            &module,
            "run",
            budget(10_000, Duration::from_secs(1), 65_536, 1)
        ),
        Err(InvocationError::LinearMemoryLimitExceeded {
            limit: 65_536,
            attempted: 131_072,
        })
    ));
}

#[test]
fn linear_memory_at_the_invocation_limit_is_allowed() {
    let sandbox = sandbox();
    let module = compile(&sandbox, r#"(module (memory 1) (func (export "run")))"#);
    let budget = budget(10_000, Duration::from_secs(1), 65_536, 1);

    let report = sandbox
        .invoke_probe(&module, "run", budget)
        .expect("one memory page is within the limit");

    assert_eq!(budget.max_linear_memory_bytes().get(), 65_536);
    assert!(report.fuel_consumed() > 0);
}

#[test]
fn runtime_linear_memory_growth_cannot_exceed_the_invocation_limit() {
    let sandbox = sandbox();
    let module = compile(
        &sandbox,
        r#"
        (module
            (memory 1 3)
            (func (export "run")
                i32.const 1
                memory.grow
                drop))
        "#,
    );

    assert!(matches!(
        sandbox.invoke_probe(
            &module,
            "run",
            budget(10_000, Duration::from_secs(1), 65_536, 1)
        ),
        Err(InvocationError::LinearMemoryLimitExceeded {
            limit: 65_536,
            attempted: 131_072,
        })
    ));
}

#[test]
fn initial_table_cannot_exceed_the_invocation_limit() {
    let sandbox = sandbox();
    let module = compile(
        &sandbox,
        r#"(module (table 2 funcref) (func (export "run")))"#,
    );

    assert!(matches!(
        sandbox.invoke_probe(
            &module,
            "run",
            budget(10_000, Duration::from_secs(1), 65_536, 1)
        ),
        Err(InvocationError::TableElementLimitExceeded {
            limit: 1,
            attempted: 2,
        })
    ));
}

#[test]
fn table_at_the_invocation_limit_is_allowed() {
    let sandbox = sandbox();
    let module = compile(
        &sandbox,
        r#"(module (table 1 funcref) (func (export "run")))"#,
    );
    let budget = budget(10_000, Duration::from_secs(1), 65_536, 1);

    let report = sandbox
        .invoke_probe(&module, "run", budget)
        .expect("one table element is within the limit");

    assert_eq!(budget.max_table_elements().get(), 1);
    assert!(report.fuel_consumed() > 0);
}

#[test]
fn runtime_table_growth_cannot_exceed_the_invocation_limit() {
    let sandbox = sandbox();
    let module = compile(
        &sandbox,
        r#"
        (module
            (table 1 3 funcref)
            (func (export "run")
                ref.null func
                i32.const 1
                table.grow
                drop))
        "#,
    );

    assert!(matches!(
        sandbox.invoke_probe(
            &module,
            "run",
            budget(10_000, Duration::from_secs(1), 65_536, 1)
        ),
        Err(InvocationError::TableElementLimitExceeded {
            limit: 1,
            attempted: 2,
        })
    ));
}
