use std::{
    num::{NonZeroU64, NonZeroUsize},
    time::Duration,
};

use wasmtime::Trap;

use super::{
    CompiledModule, InvocationBudget, InvocationBudgetError, InvocationError, SandboxCreationError,
    WasmSandbox,
};

fn fuel(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).expect("test fuel is nonzero")
}

fn memory(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("test memory limit is nonzero")
}

fn budget(fuel_value: u64, wall_time: Duration, memory_bytes: usize) -> InvocationBudget {
    InvocationBudget::new(fuel(fuel_value), wall_time, memory(memory_bytes))
        .expect("test budget is valid")
}

fn compile(sandbox: &WasmSandbox, source: &str) -> CompiledModule {
    let wasm = wat::parse_str(source).expect("test WebAssembly text is valid");
    sandbox.compile(&wasm).expect("test module compiles")
}

#[test]
fn rejects_zero_epoch_tick_and_wall_time() {
    assert!(matches!(
        WasmSandbox::new(Duration::ZERO),
        Err(SandboxCreationError::ZeroEpochTick)
    ));
    assert_eq!(
        InvocationBudget::new(fuel(1), Duration::ZERO, memory(65_536)),
        Err(InvocationBudgetError::ZeroWallTime)
    );
}

#[test]
fn finite_guest_completes_and_reports_consumed_fuel() {
    let sandbox = WasmSandbox::new(Duration::from_millis(5)).expect("sandbox starts");
    let module = compile(&sandbox, r#"(module (func (export "run") nop))"#);
    let budget = budget(10_000, Duration::from_secs(1), 65_536);

    let report = sandbox
        .invoke_probe(&module, "run", budget)
        .expect("finite guest completes");

    assert!(report.fuel_consumed() > 0);
    assert!(report.fuel_consumed() <= budget.fuel().get());
}

#[test]
fn deterministic_fuel_stops_an_infinite_guest() {
    let sandbox = WasmSandbox::new(Duration::from_millis(5)).expect("sandbox starts");
    let module = compile(
        &sandbox,
        r#"(module (func (export "run") (loop $forever br $forever)))"#,
    );
    let budget = budget(100, Duration::from_secs(5), 65_536);

    assert!(matches!(
        sandbox.invoke_probe(&module, "run", budget),
        Err(InvocationError::FuelExhausted)
    ));
}

#[test]
fn epoch_deadline_stops_an_infinite_guest_with_abundant_fuel() {
    let sandbox = WasmSandbox::new(Duration::from_millis(5)).expect("sandbox starts");
    let module = compile(
        &sandbox,
        r#"(module (func (export "run") (loop $forever br $forever)))"#,
    );
    let budget = budget(u64::MAX, Duration::from_millis(20), 65_536);

    assert!(matches!(
        sandbox.invoke_probe(&module, "run", budget),
        Err(InvocationError::DeadlineExceeded)
    ));
}

#[test]
fn ordinary_guest_traps_remain_distinct_from_resource_exhaustion() {
    let sandbox = WasmSandbox::new(Duration::from_millis(5)).expect("sandbox starts");
    let module = compile(&sandbox, r#"(module (func (export "run") unreachable))"#);
    let budget = budget(10_000, Duration::from_secs(1), 65_536);

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
    let sandbox = WasmSandbox::new(Duration::from_millis(5)).expect("sandbox starts");
    let module = compile(&sandbox, r#"(module (memory 2) (func (export "run")))"#);

    assert!(matches!(
        sandbox.invoke_probe(
            &module,
            "run",
            budget(10_000, Duration::from_secs(1), 65_536)
        ),
        Err(InvocationError::LinearMemoryLimitExceeded {
            limit: 65_536,
            attempted: 131_072,
        })
    ));
}

#[test]
fn linear_memory_at_the_invocation_limit_is_allowed() {
    let sandbox = WasmSandbox::new(Duration::from_millis(5)).expect("sandbox starts");
    let module = compile(&sandbox, r#"(module (memory 1) (func (export "run")))"#);
    let budget = budget(10_000, Duration::from_secs(1), 65_536);

    let report = sandbox
        .invoke_probe(&module, "run", budget)
        .expect("one memory page is within the limit");

    assert_eq!(budget.max_linear_memory_bytes().get(), 65_536);
    assert!(report.fuel_consumed() > 0);
}

#[test]
fn runtime_linear_memory_growth_cannot_exceed_the_invocation_limit() {
    let sandbox = WasmSandbox::new(Duration::from_millis(5)).expect("sandbox starts");
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
            budget(10_000, Duration::from_secs(1), 65_536)
        ),
        Err(InvocationError::LinearMemoryLimitExceeded {
            limit: 65_536,
            attempted: 131_072,
        })
    ));
}
