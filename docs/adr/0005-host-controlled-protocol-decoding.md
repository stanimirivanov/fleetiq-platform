# ADR 0005: Host-controlled protocol decoding

- Status: accepted
- Date: 2026-09-11

## Context

FleetIQ must support vendor protocols and customer-defined protocols from the
start. A protocol package needs enough freedom to interpret its own wire format,
identify endpoint-scoped signals, and emit several typed values from one
accepted message. It must not be able to choose trusted admission or replay
metadata such as tenant, communication device, host receipt time, or the package
release attributed to an observation.

The accepted-message contract already preserves immutable wire bytes and that
trusted context after the host's durability condition succeeds. The decoded
observation contract represents the complete output consumed by semantic
resolution. A narrow boundary is needed between them before choosing the
runtime's framing API, package manifest, serialization, or WebAssembly ABI.

Deterministic replay also requires a stable output position. If a package
reprocesses the same accepted message with the same immutable release, each
logical output must receive the same decoded-observation identity.

## Decision

Add a `fleetiq-protocol-sdk` crate that defines package-owned decoded output and
host-owned observation assembly.

A protocol package emits an ordered sequence of `DecodedOutput` values. Each
value contains:

- a device endpoint identity;
- a stable source signal identity;
- a validated decoded measurement, including its typed value, optional source
  unit, and source quality;
- an optional timestamp reported by the device.

The output deliberately omits tenant identity, communication-device identity,
host receipt time, accepted-message identity, and protocol-package provenance.
The trusted host copies those values from `AcceptedMessage` while assembling a
`DecodedObservation`. It derives the observation identity from the accepted
message and the output's zero-based position.

Batch assembly assigns indices in iterator order and reports an error if a
position cannot be represented by the observation contract's `u32` output
index. Protocol packages must emit outputs in deterministic order for a given
accepted message and immutable package release.

The SDK's Rust types define semantic behavior and provide a conformance surface
for package implementations and host adapters. They are not declared as a
stable binary or WebAssembly ABI. A future runtime adapter may serialize
equivalent values across an isolation boundary while preserving these rules.

## Consequences

- Untrusted package logic cannot attribute an output to another tenant or
  communication device, alter host receipt time, or claim a different package
  release.
- Native package tests and future sandboxed runtimes can target the same
  semantic output contract.
- One accepted message can produce observations from several endpoints and
  relayed systems without weakening admission identity.
- Replay identity remains deterministic when package output order remains
  deterministic.
- Measurement validation stays centralized in the telemetry contract rather
  than being duplicated by each runtime adapter.
- The protocol host, package lifecycle, framing, session state,
  acknowledgements, resource limits, serialization, and ABI remain explicit
  later decisions.

## Alternatives considered

### Let protocol packages construct complete decoded observations

This grants package code control over tenant, device, receipt-time, and package
provenance. Host adapters would need to validate or overwrite those fields, and
an omitted check could create a cross-tenant attribution defect.

### Include trusted context in the package output and compare it during assembly

Repeating host-owned fields expands the boundary without adding useful package
information. It also creates disagreement cases and validation paths that are
avoided when those fields cannot be supplied.

### Define a complete decoder trait now

A decoder interface depends on unresolved framing, incremental input, session
state, commands, acknowledgements, and error-recovery semantics. Freezing a
trait before those behaviors are prototyped would turn assumptions into a
public contract.

### Define the WebAssembly ABI with the first SDK types

Rust layout and Rust trait compatibility do not provide a durable plugin ABI.
Selecting a serialization and component boundary requires a measured runtime
experiment and explicit resource-limit decisions.
