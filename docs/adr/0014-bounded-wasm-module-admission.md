# ADR 0014: Bounded WebAssembly module admission

- Status: accepted
- Date: 2026-09-12

## Context

FleetIQ will accept protocol packages containing customer-authored WebAssembly.
Fuel, epoch deadlines, and linear-memory limits protect an invocation after a
module has been compiled. They do not protect the validation and compilation
path from an arbitrarily large input artifact.

The current runtime also exposes no host imports. A module can nevertheless
declare imports, compile successfully, and fail only when the host attempts to
instantiate it with an empty import list. Discovering this fixed capability
mismatch during message processing produces a late and misleading runtime
failure.

The experiment needs an explicit admission boundary without defining package
manifests, signatures, distribution, activation state, or the stable protocol
ABI. Wasmtime validates binary input as part of `Module::new`; calling
`Module::validate` first would repeat validation and still leave compilation as
a separate unbounded operation. See the Wasmtime
[`Module` documentation](https://docs.rs/wasmtime/48.0.2/wasmtime/struct.Module.html).

## Decision

Introduce `ModuleAdmissionPolicy`, which contains a nonzero maximum size for the
raw core WebAssembly module in bytes. `WasmSandbox::compile` requires this policy
for every module.

The sandbox compares the input length with the configured limit before passing
the slice to Wasmtime. Input larger than the limit returns
`ModuleTooLarge { limit, actual }`; a module exactly at the limit remains
eligible. The size check deliberately precedes format validation, so an
oversized malformed input receives the bounded-admission result without parser
or compiler work.

Inputs within the limit are passed once to `Module::new`, which validates and
compiles a core WebAssembly binary under the engine's configured features.
Wasmtime failures are retained as `Compilation` errors with their original
source.

After successful compilation, the host inspects the module's declared imports.
The current runtime exposes no host capabilities, so any declared import returns
`HostImportsNotAllowed { count }` during admission. This converts a permanent
module/host incompatibility into an activation-time diagnostic. Future imports
require a focused capability contract and must revise this policy deliberately.

The byte limit bounds input presented to Wasmtime, but it is not a compilation
time, compiler-memory, or compiled-code-size limit. A future activation worker
must isolate compilation from telemetry processing, bound concurrency, and
measure time and memory before production package admission is complete.

## Consequences

- Oversized raw modules are rejected before Wasmtime parsing or compilation.
- Every compilation call must state its host-owned size policy explicitly.
- A module at the exact byte limit is handled normally rather than rejected by
  an off-by-one boundary.
- Invalid or unsupported modules remain distinguishable from size violations.
- Modules requesting unavailable host capabilities fail during admission rather
  than during a device-message invocation.
- Wasmtime receives accepted bytes once; the runtime does not validate and then
  parse the same module again solely to create the compiled form.
- The policy does not bound compilation duration, compiler allocations,
  generated native code, package metadata, or aggregate activation throughput.
- The current API admits only raw core WebAssembly modules. It does not establish
  the final package artifact format.

## Alternatives considered

### Check size in an upload or API handler only

Other callers, recovery jobs, tests, or future package sources could bypass the
transport-specific check. Requiring the policy at the compilation boundary
keeps the invariant next to the expensive operation.

### Call `Module::validate` before `Module::new`

This provides an explicit validation step, but `Module::new` validates again.
It duplicates work without bounding compilation time or memory. The raw-byte
check supplies the useful pre-compilation guard.

### Allow imported modules to fail during instantiation

The current host always supplies an empty import list. Deferring this known
incompatibility makes a package appear activatable and turns it into a repeated
runtime failure for device traffic.

### Add provisional host imports now

An import namespace and function signatures become part of the package
compatibility boundary. They should follow the stable invocation and
serialization design rather than emerge from the artifact-size guard.

### Treat the raw-byte cap as a complete compilation budget

Modules of the same size can have materially different validation and code
generation costs. Input size is one necessary bound, not a substitute for
activation isolation and measurement.

## Out of scope

- package manifests and archive formats;
- package identity, versions, signatures, and trust roots;
- artifact persistence, distribution, activation, rollback, and caching;
- compilation wall-clock deadlines and cancellation;
- compilation concurrency and worker isolation;
- compiler-memory and compiled-code-size limits;
- decompression limits for a future package container;
- stable exports, imports, serialization, or protocol ABI;
- WebAssembly components and WASI;
- tenant-specific admission policies and production defaults;
- protocol-specific package implementations.
