# ADR 0016: Explicit WebAssembly stack limits

- Status: accepted
- Date: 2026-09-12

## Context

The protocol-package runtime limits instruction fuel, elapsed execution, linear
memory, and table elements. Recursive or deeply nested guest calls consume stack
outside WebAssembly linear memory. A package can therefore exhaust stack without
crossing either guest allocation limit.

Wasmtime traps WebAssembly stack overflow and exposes `Config::max_wasm_stack`.
The setting defaults to 512 KiB, but that value belongs to Wasmtime and may
change independently of FleetIQ policy. Leaving it implicit also prevents
operators and tests from knowing which ceiling a sandbox uses. See Wasmtime's
[`Config::max_wasm_stack` documentation](https://docs.rs/wasmtime/48.0.2/wasmtime/struct.Config.html#method.max_wasm_stack).

The Wasmtime setting has important limits. Its byte value is approximate, it is
configured on the engine rather than an individual store, and it limits stack
consumed while executing WebAssembly. It does not ensure that the calling
native thread has that much stack remaining. Exhausting the native thread stack
can abort the process before a WebAssembly trap is delivered.

## Decision

Require a nonzero maximum WebAssembly stack size when constructing
`WasmSandbox`. The sandbox passes the value to `Config::max_wasm_stack` before
creating its engine, retains the configured value, and exposes it for
diagnostics.

Stack policy is sandbox-wide rather than part of `InvocationBudget`. Wasmtime
configures the guard on the engine, and every store and module associated with
that engine shares it. Supporting different stack policies therefore requires
separate sandbox engines.

Classify Wasmtime's `StackOverflow` trap as
`StackLimitExceeded { configured_limit }`. The error carries the configured
approximate ceiling because Wasmtime does not report the exact stack consumption
that triggered the trap. Fuel exhaustion, epoch interruption, guest allocation
limits, and unrelated traps retain their existing classifications.

A recursive fixture runs with abundant fuel and a long deadline and proves that
the explicit 64 KiB test ceiling produces the typed stack-limit result.

## Consequences

- Every sandbox states its WebAssembly stack policy explicitly instead of
  inheriting Wasmtime's default.
- Recursive and deeply nested guest calls terminate with a distinguishable
  invocation result.
- The configured limit is available for diagnostics and runtime metrics.
- Modules compiled for one sandbox share that sandbox's stack policy during
  invocation.
- A different stack policy requires another engine and may reduce compiled
  module cache sharing.
- The limit is approximate and should not be presented as exact consumed bytes.
- The limit does not reserve or bound the calling thread's native stack.
- Future host imports can consume native stack. Direct host stack consumption is
  not stopped by this setting, although a later callback into WebAssembly may
  observe that the guest stack guard has been crossed.
- Production execution still needs threads or asynchronous fibers with known
  stack capacity, conservative configured limits, and process-level isolation
  appropriate to untrusted extensions.

## Alternatives considered

### Keep Wasmtime's default

The default currently provides a guard, but it is an implicit dependency on a
runtime-library choice rather than an explicit FleetIQ operational policy.

### Put the stack value in `InvocationBudget`

Wasmtime configures maximum WebAssembly stack on `Engine`, not `Store`. A
per-invocation field would imply isolation that the implementation cannot
provide.

### Treat fuel or deadlines as sufficient

A recursive guest normally consumes fuel and time, but stack can be exhausted
before either budget. The limits protect different resources and should produce
different diagnostics.

### Report stack overflow as a generic runtime trap

Stack exhaustion is an expected resource-policy outcome. Classifying it
separately supports package diagnostics, metrics, and operator decisions.

### Claim the setting protects the native thread stack

Wasmtime explicitly does not provide that guarantee. Making such a claim could
hide a process-abort risk when callers use undersized or already-consumed native
stacks.

## Out of scope

- native thread stack sizing and supervision;
- process isolation and operating-system resource controls;
- asynchronous Wasmtime execution and fiber stack configuration;
- host imports and their stack consumption;
- per-tenant or per-invocation stack variation within one engine;
- production stack-size defaults and capacity measurements;
- stable package exports, serialization, and ABI;
- package activation, caching, signatures, and distribution;
- protocol-specific package implementations.
