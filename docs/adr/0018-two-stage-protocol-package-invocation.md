# ADR 0018: Use a two-stage protocol package invocation contract

- Status: accepted
- Date: 2026-09-12

## Context

FleetIQ already has focused semantic contracts for incremental frame detection,
opaque session-state replacement, decoded device outputs, and host-gated reply
proposals. Those contracts constrain individual package capabilities, but they
do not yet describe the complete input and output of one package operation.

The invocation boundary must support stateful custom protocols without giving
untrusted package code ownership of transport buffers, sockets, admission
identity, or provenance. It must also preserve deterministic ordering when one
frame produces several decoded values or several replies.

Framing and frame processing have different retry behavior. The host may ask a
package to inspect the same incomplete byte prefix repeatedly as more transport
data arrives. Allowing that operation to change state or propose replies would
make retries duplicate side effects. Once the host extracts a complete frame,
processing it may legitimately decode values, advance handshake state, and
propose protocol responses together.

The current Wasmtime crate still uses an admitted probe export. Choosing binary
serialization, memory ownership, export names, or ABI versioning before the
semantic operation is explicit would couple domain behavior to an experimental
transport mechanism.

## Decision

Define two package operations at the semantic Rust boundary:

1. `FramingInput` lends the package all unread transport bytes and the current
   opaque session state. The package returns only the existing `FrameBoundary`.
   It cannot propose state changes, decoded values, or replies during framing.
2. `FrameProcessingInput` lends the package exactly one extracted frame and the
   session state in effect before processing. The package returns one
   `FrameProcessingOutput` containing its ordered decoded outputs, one complete
   session-state action, and its ordered reply proposals.

Both inputs borrow host-owned bytes. Their routine debug representations expose
lengths without payload contents. Neither input contains tenant, device,
package-release, receipt-time, accepted-message, or other trusted provenance.

Decoded-output order remains part of deterministic observation identity. Reply
order becomes the transport write order among proposals that are eligible for
release. Either sequence may be empty, so handshake, keepalive, and
acknowledgement-only frames do not require synthetic telemetry.

Treat the processing output as one proposed effect set. The host validates the
entire result against state, reply, output-count, and semantic policies before
applying any part. Invocation failure or validation failure preserves prior
state, emits no decoded output, and releases no reply. After validation, each
reply still observes its declared release condition.

Keep this contract independent of dispatch. Do not introduce a Rust package
trait because a Rust trait is not the future WebAssembly compatibility boundary,
and the repository does not yet have multiple native implementations that need
dynamic dispatch.

## Consequences

- Repeated framing polls are free of package-proposed side effects.
- Stateful framing remains possible because the package can inspect, but cannot
  mutate, the current opaque state.
- One complete frame can produce any combination of telemetry, state
  transition, immediate protocol response, and durability-gated response.
- Ordered collections make replay identity and reply sequencing explicit.
- The aggregate gives the host one validation boundary before effects become
  visible.
- Package code cannot claim trusted identity or timing provenance through the
  invocation input.
- Native conformance fixtures and the eventual isolated runtime can share the
  same semantic operation without committing to Rust dynamic dispatch.
- The WebAssembly adapter must later encode these inputs and outputs while
  retaining their ordering and all-or-nothing validation behavior.

## Alternatives considered

### Use one operation for arbitrary unread input

Combining frame discovery and processing would require the package to say how
many bytes it consumed while also returning effects. Incomplete input and
retries would then need additional rules to prevent state or reply duplication.

### Permit effects during framing

A stateful protocol can inspect its prior state to locate a boundary, but
changing state while input remains incomplete makes repeated polls observable.
Effects belong to processing of an extracted frame.

### Pass tenant and device context to the package

Protocol decoding does not need authority to select trusted ownership or
provenance. Keeping those fields in the host prevents a package from
misattributing outputs and keeps the contract reusable across admission models.

### Return decoded outputs, state, and replies through separate calls

Separate calls lose the single validation boundary and make partial application
more likely when a later result fails or exceeds a limit.

### Define a Rust protocol-package trait

A trait would suggest a native plugin boundary without solving WebAssembly
serialization or compatibility. Concrete host and package adapters can use the
semantic data types directly until repeated implementation needs justify a
consumer-owned trait.

### Define the WebAssembly ABI in this change

Serialization, allocation, memory transfer, export versioning, and malformed
guest-result handling need their own focused design. This decision establishes
the behavior that ABI must carry first.

## Out of scope

- binary serialization and compatibility versioning;
- stable WebAssembly export names and function signatures;
- guest-memory allocation and transfer conventions;
- host validation limits for aggregate output counts or bytes;
- orchestration that applies state, assembles observations, and stages replies;
- accepted-message durability and transaction boundaries;
- transport session lifecycle, identification, and package selection;
- package manifests, signing, activation, rollback, and distribution;
- protocol-specific package implementations.
