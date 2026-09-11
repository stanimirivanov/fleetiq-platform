use std::{num::NonZeroU64, time::Duration};

use wasmtime::Trap;

use super::{
    CompiledModule, InvocationBudget, InvocationBudgetError, InvocationError, SandboxCreationError,
    WasmSandbox,
};

fn fuel(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).expect("test fuel is nonzero")
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
        InvocationBudget::new(fuel(1), Duration::ZERO),
        Err(InvocationBudgetError::ZeroWallTime)
    );
}

#[test]
fn finite_guest_completes_and_reports_consumed_fuel() {
    let sandbox = WasmSandbox::new(Duration::from_millis(5)).expect("sandbox starts");
    let module = compile(&sandbox, r#"(module (func (export "run") nop))"#);
    let budget =
        InvocationBudget::new(fuel(10_000), Duration::from_secs(1)).expect("budget is valid");

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
    let budget = InvocationBudget::new(fuel(100), Duration::from_secs(5)).expect("budget is valid");

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
    let budget =
        InvocationBudget::new(fuel(u64::MAX), Duration::from_millis(20)).expect("budget is valid");

    assert!(matches!(
        sandbox.invoke_probe(&module, "run", budget),
        Err(InvocationError::DeadlineExceeded)
    ));
}

#[test]
fn ordinary_guest_traps_remain_distinct_from_resource_exhaustion() {
    let sandbox = WasmSandbox::new(Duration::from_millis(5)).expect("sandbox starts");
    let module = compile(&sandbox, r#"(module (func (export "run") unreachable))"#);
    let budget =
        InvocationBudget::new(fuel(10_000), Duration::from_secs(1)).expect("budget is valid");

    let Err(InvocationError::Runtime(error)) = sandbox.invoke_probe(&module, "run", budget) else {
        panic!("an ordinary guest trap should remain a runtime failure");
    };

    assert_eq!(
        error.downcast_ref::<Trap>(),
        Some(&Trap::UnreachableCodeReached)
    );
}
