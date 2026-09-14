# ADR 0027: Poll component framing through the host input buffer

- Status: accepted
- Date: 2026-09-14

## Context

FleetIQ has a host-owned bounded input buffer and an admitted WebAssembly
component operation for locating the first complete frame. Calling these APIs
separately would expose a raw frame-boundary decision and require every session
implementation to preserve the relationship between the bytes inspected by the
component and the bytes later removed by the host.

Framing is intentionally side-effect-free. The package may inspect current
session state but cannot change it, emit decoded observations, or propose
replies. A framing failure, need-more-data result, or invalid reported length
must retain all unread bytes so session policy can decide whether to wait,
quarantine, retry, or close the connection.

The invocation resource report remains operationally useful for successful
need-more-data and frame decisions, and when the component completes but reports
a frame longer than its input.

## Decision

Add `WasmSandbox::poll_component_frame` as the public component framing boundary.
It accepts:

- an admitted `CompiledProtocolComponent`;
- an invocation budget;
- the current host-owned `SessionState`; and
- the mutable host-owned `SessionInputBuffer`.

While holding exclusive access to the input buffer, the adapter invokes
`locate-frame` with the buffer's current unread bytes and the session's current
state bytes. It then passes the returned semantic `FrameBoundary` to
`SessionInputBuffer::poll_frame`. No code can mutate the buffer between component
inspection and host extraction.

Return `ProtocolFramePoll` containing the host-owned `FramePoll` and invocation
resource report. The outcome is either `NeedMoreData` or one extracted
`WireFrame`; each call extracts at most one frame and leaves coalesced following
bytes for another poll.

Return `ProtocolFramePollError` for:

- `Invocation`, retaining package rejection, invalid lifted output, resource
  exhaustion, traps, and other component failures;
- `FrameLengthExceedsBuffer`, retaining requested and available byte counts plus
  the completed invocation report.

Restrict the raw component framing invocation and its generic output wrapper to
the WebAssembly adapter. External callers use the host-buffer integration so a
component decision cannot bypass length validation or prefix ownership.

## Consequences

- The component inspects exactly the unread bytes from which the host may extract
  a frame.
- Current host-owned session state is available to state-dependent framing
  without allowing framing to mutate that state.
- Need-more-data, package rejection, invocation failure, invalid semantic output,
  and impossible frame lengths preserve all unread bytes.
- A successful frame decision removes exactly one validated prefix and retains
  coalesced following input.
- Framing resource usage remains available for metrics and package-health policy.
- Raw component frame-boundary decisions are no longer part of the public API.
- The operation still does not process, durably accept, or acknowledge the
  extracted frame.

## Alternatives considered

### Let session code invoke and poll separately

This duplicates orchestration and permits the inspected input to diverge from
the buffer later consumed. It also makes it easier to forget host frame-length
validation or lose invocation accounting.

### Let the component consume the input buffer directly

Giving guest code buffer ownership would make partial consumption possible on a
trap or package error and would let protocol code discard evidence before host
policy decides how to handle failure.

### Poll and process every coalesced frame in one call

A loop would combine multiple invocation budgets, processing results, and
failure points. Extracting at most one frame keeps resource accounting and
session scheduling explicit and prevents one connection from monopolizing a
worker turn.

### Allow framing to update session state

Repeated polling of the same incomplete prefix could then duplicate state
transitions. State changes and replies remain effects of processing one complete
frame.

## Out of scope

- automatically processing the extracted `WireFrame`;
- looping over all coalesced frames;
- transport reads, connection scheduling, backpressure, and fairness;
- durable raw-frame acceptance and broker acknowledgement;
- decoded-observation assembly and persistence;
- reply release and transport writes;
- session failure and quarantine policy;
- package selection, pinning, upgrades, and rollback;
- production invocation budgets and tenant-specific overrides.
