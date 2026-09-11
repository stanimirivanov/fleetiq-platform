# ADR 0012: Wasmtime invocation interruption experiment

- Status: accepted
- Date: 2026-09-11

## Context

FleetIQ intends to support customer-authored protocol packages. Package code may
contain an accidental infinite loop or deliberately consume excessive compute.
A process boundary or WebAssembly sandbox alone does not bound execution time;
the host needs explicit interruption mechanisms for every invocation.

The Phase 0 plan identifies Wasmtime as a candidate rather than a predetermined
runtime. Before designing the package ABI, the project must verify that the
pinned Rust toolchain and a current stable Wasmtime release can stop runaway
guest code for two distinct reasons:

- deterministic work exceeding a configured instruction budget;
- execution exceeding an external wall-clock deadline.

These limits serve different purposes. Fuel produces repeatable compute
accounting but adds instrumentation overhead. Epoch interruption is cheaper and
lets the host interrupt running WebAssembly from outside the store, but its
timing depends on ticker and operating-system scheduling. Wasmtime documents
both mechanisms in its
[interruption examples](https://docs.wasmtime.dev/examples-interrupting-wasm.html)
and [`Config` API](https://docs.rs/wasmtime/48.0.2/wasmtime/struct.Config.html).

## Decision

Add an isolated `fleetiq-protocol-wasm` crate using the stable Wasmtime 48 line.
Disable Wasmtime's default feature set and enable only error integration, the
Cranelift compiler, the core runtime, and standard-library support required by
this experiment. The component model, WASI, cache, profiling, pooling allocator,
and other optional capabilities are not enabled.

`WasmSandbox` creates an engine with both fuel consumption and epoch
interruption enabled. It owns one background epoch ticker rather than starting a
timer thread per invocation. Dropping the sandbox signals and joins that ticker.

Each invocation receives an `InvocationBudget` containing:

- a nonzero Wasmtime fuel allowance;
- a nonzero wall-clock allowance.

The sandbox creates a fresh `Store` for the invocation, installs the fuel, and
sets an epoch deadline relative to the engine's current epoch. Wall time is
rounded up to the sandbox's configured epoch resolution and therefore may
overshoot by approximately one tick plus scheduler delay.

The runtime classifies Wasmtime's `OutOfFuel` trap as `FuelExhausted` and its
epoch `Interrupt` trap as `DeadlineExceeded`. Other guest traps, missing or
incompatible exports, and instantiation failures remain ordinary runtime errors.
A successful invocation reports consumed fuel.

WebAssembly compilation happens separately from message invocation. Packages
can therefore be validated and compiled during activation rather than charging
compilation latency to each device message. Artifact size, compilation
concurrency, cache policy, and activation time need separate limits.

The crate exposes a temporary no-argument probe export solely to exercise
instantiation and interruption. Its shape is not the protocol package ABI.
Fixtures prove that a finite guest completes, deterministic fuel stops an
infinite loop, epoch advancement stops the same loop when fuel is abundant, and
an ordinary guest trap remains distinguishable from resource exhaustion.

## Consequences

- Wasmtime can enforce both deterministic and wall-clock compute limits with the
  repository's pinned Rust toolchain.
- A runaway guest cannot monopolize the invoking thread indefinitely while it
  executes WebAssembly instructions and the epoch ticker remains healthy.
- Fuel values are suitable for deterministic conformance fixtures, but they are
  Wasmtime accounting units rather than portable CPU-time measurements.
- Epoch deadlines are approximate and require monitoring the shared ticker's
  liveness in a production host.
- A fresh store prevents state and accumulated instances from leaking between
  probe invocations. Protocol session state remains in the separately bounded
  host-owned contract.
- No host imports are exposed, so the experiment grants the guest no filesystem,
  network, clock, environment, or WASI capabilities.
- Fuel and epochs interrupt running WebAssembly. They do not interrupt a
  blocking synchronous host function. Future host imports must be nonblocking,
  bounded independently, or integrated through an asynchronous execution model.
- Wasmtime and Cranelift add meaningful compile time and binary size. The
  reduced feature set limits that cost, and Phase 0 measurements must still
  compare it with deployment constraints.

## Alternatives considered

### Rely on WebAssembly isolation without execution limits

Memory safety and capability isolation do not stop an infinite loop. One guest
could monopolize a gateway worker indefinitely.

### Use only fuel

Fuel gives deterministic work limits, but its instrumentation has higher
runtime overhead. A wall-clock boundary also protects against an unexpectedly
expensive instruction mix and host scheduling requirements.

### Use only epoch interruption

Epoch timing is nondeterministic and does not provide a repeatable work budget
for fixtures, capacity policy, or diagnosing packages that consume excessive
compute before a deadline.

### Wrap synchronous guest execution in a thread timeout

Timing out the waiting caller does not stop the guest thread. It can leak
runaway work and eventually exhaust the process's threads and CPU.

### Define the protocol component ABI in this experiment

Framing, state, replies, decoded outputs, commands, and serialization still need
to be composed into one invocation model. A temporary probe proves interruption
without making those unresolved choices part of a compatibility boundary.

## Out of scope

- the stable protocol package ABI and serialization;
- WebAssembly component-model selection;
- WASI and all other host imports;
- asynchronous guest execution;
- blocking-host-call cancellation;
- linear-memory, table, instance, and aggregate process-memory limits;
- package artifact-size and compilation-time limits;
- compiled-module cache and distribution;
- package signatures, activation, version pinning, and rollback;
- production epoch-ticker supervision and metrics;
- scheduling, concurrency, and store pooling;
- integration with framing, session state, replies, or decoded outputs;
- protocol-specific WebAssembly packages.
