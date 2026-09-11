# ADR 0009: Bounded incremental protocol framing

- Status: accepted
- Date: 2026-09-11

## Context

Stream transports do not preserve application message boundaries. One read may
contain part of a frame, one complete frame, or several frames. Binary trackers
often declare a payload length in a header, while custom protocols may use a
terminator or another package-specific rule. Treating a transport read as a
message would therefore corrupt valid fragmented and coalesced input.

FleetIQ must let protocol packages recognize their own framing without giving
package code ownership of transport sockets or unbounded session memory. The
host must also reject impossible framing decisions before bytes enter the
durable accepted-message pipeline.

The complete runtime boundary remains under investigation. Session state,
identification, acknowledgements, commands, runtime isolation, and execution
budgets will affect its eventual trait or component interface. Incremental
framing can be proved independently without freezing that larger API.

## Decision

The `fleetiq-protocol-sdk` crate defines `FrameBoundary`, a narrow package
decision with two outcomes:

- `NeedMoreData` means the borrowed input prefix is incomplete;
- `Frame { length }` gives the nonzero byte length of the first complete frame.

The package receives a borrowed slice. It cannot consume, replace, retain, or
allocate the host's unread transport buffer through this contract.

The new `fleetiq-protocol-host` crate owns `SessionInputBuffer`. Each session
buffer has a nonzero, host-configured hard limit. An append that would cross the
limit fails atomically and preserves all previously accepted bytes. To poll for
a frame, the host passes the complete unread prefix to package logic, validates
the returned length, extracts only that prefix, and retains any coalesced bytes
for the next poll.

`NeedMoreData`, package errors, and lengths larger than the available input do
not mutate the buffer. The caller can therefore apply an explicit connection or
diagnostic policy after a failure. Extracted bytes are represented as a
`WireFrame`; its debug representation reports the length without exposing
payload contents.

Tests use both a length-prefixed binary fixture and newline-delimited structured
fixtures. They verify fragmented input, coalesced frames, the hard buffer limit,
invalid package decisions, and package failures.

Framing occurs before durable admission. A `WireFrame` is not an
`AcceptedMessage`. The ingestion host must satisfy the configured durability
condition before constructing or publishing an accepted message and before
releasing any device acknowledgement tied to that condition.

This decision defines semantic Rust types for the experiment. It does not
declare a stable Rust decoder trait, serialization format, or WebAssembly ABI.

## Consequences

- Protocol packages can implement binary, text, and customer-defined framing
  while the host keeps transport and memory ownership.
- Arbitrary transport fragmentation and coalescing no longer affect logical
  frame identity.
- A stalled or malicious package cannot make unread input grow beyond the
  configured per-session limit through repeated `NeedMoreData` decisions.
- Package framing errors do not silently discard evidence. A later session
  policy must decide whether to retry, record diagnostics, or disconnect.
- The host currently copies an extracted frame into independent owned storage.
  This keeps ownership simple for the experiment; profiling may justify a
  segmented or reference-counted buffer later.
- Additional runtime work is still required before accepting live device
  connections.

## Alternatives considered

### Treat every transport read as one frame

This contradicts stream transport behavior. Valid frames can be split across
reads, and multiple frames can arrive in one read.

### Let each package own its session buffer

Package-owned buffering weakens central memory limits and makes transport input
retention, error handling, and observability inconsistent between protocols.

### Let a package return replacement bytes or a consumed prefix

Allowing a package to mutate host input makes it harder to preserve exact wire
evidence and validate what was framed. Reporting a length is sufficient for the
host to perform the mutation itself.

### Define the full decoder trait in this increment

The required behavior for session state, endpoint identification,
acknowledgements, commands, and failures is not yet proven. A full trait would
couple this focused framing experiment to those unresolved choices.

### Define the WebAssembly boundary now

Runtime selection also requires measured decisions for serialization, memory,
fuel, timeouts, component lifecycle, and observability. The semantic framing
decision can remain valid across several possible ABI designs.

## Out of scope

- transport listeners and connection lifecycle;
- protocol manifests and package selection;
- host-controlled protocol session state;
- endpoint or communication-device identification;
- decoding frames into observations;
- acknowledgements and device commands;
- durable log integration;
- time, fuel, and memory enforcement inside a package runtime;
- native or WebAssembly package loading;
- protocol-specific implementations such as Teltonika codecs.
