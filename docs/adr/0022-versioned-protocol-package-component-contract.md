# ADR 0022: Use a versioned WIT protocol-package contract

- Status: accepted
- Date: 2026-09-13

## Context

FleetIQ must support independently authored device protocols without rebuilding
the fleet-domain application. The semantic SDK already defines side-effect-free
framing followed by atomic frame processing. The Wasmtime experiments establish
execution, memory, table, stack, module-admission, and opaque byte-transfer
limits, but their host-selected core-module exports deliberately do not define a
package compatibility boundary.

A production boundary must express lists, records, variants, strings, and
opaque bytes without relying on Rust memory layout. It must support package
authors using languages other than Rust and make incompatible changes explicit.
Maintaining a separate application serialization protocol over core WebAssembly
would also require FleetIQ to specify allocation, representation, code
generation, and compatibility behavior already addressed by the WebAssembly
Component Model.

## Decision

Use a WebAssembly Interface Type document as the authoritative protocol-package
contract. Its initial package identity is
`fleetiq:protocol-package@0.1.0`, and its exported world is
`protocol-package`.

The interface exposes two operations:

1. `locate-frame` receives unread transport bytes and current opaque session
   state, then returns either `need-more-data`, a frame byte length, or a bounded
   package error code;
2. `process-frame` receives exactly one complete frame and current state, then
   returns all decoded outputs, a complete state action, and all proposed
   replies as one result, or a bounded package error code.

The WIT fields mirror the accepted semantic SDK boundary. They do not expose
tenant, admitted device, package release, receipt time, sockets, durable storage,
credentials, clocks, filesystems, or secrets to package code.

Represent arbitrary-precision decimal values as base-ten strings because WIT
has only fixed-width numeric primitives and FleetIQ domain values use exact
decimals. The future host adapter must parse them through the domain constructor.
All WIT output remains untrusted: the adapter and existing host gates validate
frame length, identifiers, units, measurement combinations, timestamps,
nonempty replies, state size, collection counts, and byte totals.

Use a package-defined `u32` error code instead of a package-controlled diagnostic
string. Package release metadata can document codes, while the runtime retains a
fixed-size value for metrics and session policy. Component traps and host limit
failures remain distinct runtime error classes.

Generate Wasmtime bindings from the checked-in WIT directory during normal Rust
compilation. This makes WIT parsing and type generation part of the quality gate.
Expose the WIT source and its package/world identity for later SDK publication,
while keeping generated host bindings private until admission and invocation
policies are implemented.

The `0.x` version marks a pre-production interface. Backwards-compatible edits
advance the minor version; incompatible edits require a new major version once
the contract reaches `1.0.0`. A package session will eventually pin one admitted
component and exact supported world version.

## Consequences

- Protocol packages receive a language-neutral, machine-checkable contract.
- The Component Model canonical ABI owns value lowering, lifting, and memory
  representation instead of a FleetIQ-specific serialization format.
- Framing remains unable to propose state, telemetry, or reply effects.
- Processing retains output and reply order and proposes state atomically.
- Exact decimal values cross the interface without binary floating-point loss.
- Package-controlled routine errors have a bounded representation.
- Every host build validates the authoritative WIT document and generated
  bindings against the pinned Wasmtime version.
- Enabling Wasmtime's Component Model adds compile-time dependencies and binary
  code that must be included in later runtime measurements.
- Existing core-module probe and transfer experiments remain useful evidence for
  resource policy, but they are not the package compatibility boundary.

## Alternatives considered

### Serialize SDK values through the core-module transfer exports

CBOR, Protocol Buffers, or another format could encode the values, but FleetIQ
would own an additional schema, allocator ABI, language SDK generation, and
compatibility layer. The Component Model already defines language-neutral typed
interfaces and their canonical ABI.

### Expose Rust structs and traits directly

Rust layout and trait ABIs are not stable cross-language plugin contracts. That
would also encourage loading customer code natively into the host process.

### Use JSON across the opaque byte-transfer experiment

JSON is easy to inspect but does not natively distinguish opaque byte strings,
adds textual conversion overhead, and still needs a separate schema and ABI
versioning convention.

### Include diagnostic strings in package errors

An untrusted package could create large or sensitive routine log values. A
numeric code is bounded and can be resolved through trusted package metadata.

### Expose trusted admission context to the package

Tenant and device identity are established by the host. Passing them through the
package contract would let protocol parsing depend on trusted provenance and
would enlarge the interface without a demonstrated decoding need.

## Out of scope

- component binary admission and embedded-world compatibility checks;
- component instantiation and invocation;
- conversion between generated WIT values and semantic SDK types;
- guest SDK generation, packaging, registry publication, and conformance tests;
- package manifests and supported-host-version negotiation;
- package error-code metadata and operator diagnostics;
- component pooling, pre-instantiation, reset, and concurrency policy;
- WASI and custom host imports;
- production resource-limit values and performance benchmarks;
- protocol-specific Teltonika or synthetic package implementations.
