# ADR 0024: Admit and pre-bind exact protocol components

- Status: accepted; public artifact identity amended by [ADR 0038](0038-bind-admitted-components-to-release-and-artifact.md)
- Date: 2026-09-13

## Context

FleetIQ has a versioned WIT world and conversion between generated component
values and semantic SDK types. It still needs an admission boundary that proves
an uploaded artifact is a supported WebAssembly component before activation or
device traffic can reach it.

Wasmtime can validate and compile Component Model binaries, while generated
bindings can check whether a component implements the exact interface expected
by the host. Deferring those checks until each invocation would mix permanent
package compatibility failures with transient session failures and repeat
export discovery on the message path.

Component compilation consumes host CPU and memory outside guest execution.
Invocation fuel, epoch deadlines, linear-memory limits, and table limits do not
bound compilation. At minimum, untrusted artifact size must be checked before
passing bytes to Wasmtime.

The initial component world requires no host capabilities. A component import is
therefore a permanent compatibility failure rather than something the runtime
should satisfy implicitly.

## Decision

Add a `ComponentAdmissionPolicy` with an explicit nonzero maximum raw component
size. `WasmSandbox::compile_protocol_component` performs admission in this order:

1. reject bytes exceeding the configured ceiling before parsing or compilation;
2. validate and compile the bytes as a Component Model component using the
   sandbox's configured Wasmtime engine;
3. inspect the component type and reject every top-level host import;
4. pre-instantiate it with an empty component linker;
5. construct generated `ProtocolPackagePre` bindings for the exact WIT world.

Return an opaque `CompiledProtocolComponent` only after all checks pass. It
retains the generated pre-instantiation bindings, which include the compiled
component and bound export indices. Later invocation code can instantiate this
artifact without repeating string-based export discovery or world type checks.

Classify expected admission failures separately:

- `ComponentTooLarge` reports configured and actual byte counts;
- `Compilation` retains Wasmtime's validation or compilation source;
- `HostImportsNotAllowed` reports the number of requested top-level imports;
- `InterfaceMismatch` retains the generated-binding or linker type-check source.

Do not expose the component, linker, generated bindings, export indices, or raw
bytes through the public artifact. Its debug representation reports only its
type and remains non-exhaustive.

Use WIT-generated dummy components in tests. This exercises Wasmtime's actual
Component Model validation and generated binding checks for matching, imported,
and incompatible worlds without checking in opaque binary fixtures.

## Consequences

- Oversized component input never reaches Wasmtime.
- Core WebAssembly modules and malformed components fail component compilation.
- Components cannot gain host capabilities merely by declaring imports.
- Missing operations and incompatible WIT signatures fail during admission.
- A successfully admitted artifact is bound to the host's exact generated world
  before it can be activated.
- Message handling can reuse typed pre-instantiation metadata rather than
  repeating compatibility discovery.
- Compilation and pre-instantiation remain outside invocation resource budgets.
- The component artifact byte ceiling is independent from the earlier
  core-module admission ceiling so deployments can measure and configure them
  separately.
- Test-only WIT tooling increases development dependency compilation time but
  produces readable fixtures from the authoritative interface definition.

## Alternatives considered

### Reuse core-module admission

Core modules and components have different binary structure, type systems,
imports, and binding mechanisms. Reusing `ModuleAdmissionPolicy` would obscure
which artifact is accepted and couple independently measured byte ceilings.

### Check only the component's package or world name

Names do not prove record fields, variants, function parameters, return types,
or interface versions are compatible. Generated typed bindings validate the
complete structural contract.

### Discover exports during every invocation

Interface compatibility is invariant for a compiled artifact. Repeating lookup
adds work and turns an activation defect into a device-session failure.

### Permit imports and fail when linking

An empty linker would eventually reject unresolved imports, but an explicit
admission error states the current no-capability policy and reports the requested
surface before binding exports.

### Instantiate once during admission

Instantiation can execute component initialization and allocate guest resources.
It belongs under an invocation or lifecycle budget. Pre-instantiation performs
linking and type checks without creating session-visible guest state.

### Store only the compiled component

That would require later code to recreate the linker and rediscover typed
exports. Retaining generated pre-bindings records precisely what admission
approved.

## Out of scope

- component instantiation and operation invocation;
- fuel, epoch, memory, table, and stack enforcement during component calls;
- classification of component traps and package-declared errors;
- applying semantic processing-output limits;
- package manifests, signatures, provenance, and artifact storage;
- activation permissions, rollout, session pinning, upgrades, and rollback;
- compilation worker isolation, timeouts, concurrency, and caching;
- WASI and explicit host capability imports;
- component pooling, reuse, and reset protocols;
- guest SDK publication and protocol-specific packages.
