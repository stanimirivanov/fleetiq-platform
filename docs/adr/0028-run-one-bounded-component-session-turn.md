# ADR 0028: Run one bounded protocol component session turn

- Status: accepted
- Date: 2026-09-14

## Context

FleetIQ can ask an admitted protocol component to locate at most one frame in a
host-owned input buffer. It can also process an already extracted frame and pass
the component's complete proposal through the host processing gate. Session
code would otherwise have to compose these operations itself and preserve their
state, ownership, resource-accounting, and failure rules at every call site.

Framing and processing remain separate component invocations because they have
different retry and side-effect behavior. They also require independent
resource budgets: incomplete input can cause framing to run many times, while
processing runs once for each extracted frame. A combined host turn must retain
this separation without permitting the session state to change between the two
operations.

Once framing succeeds, the input prefix has left the connection buffer. A later
processing failure must not silently lose that frame or put it back ahead of
bytes that may already represent another complete frame. The caller needs the
exact extracted bytes to apply its retry, quarantine, evidence-retention, or
connection-closure policy.

## Decision

Add `WasmSandbox::run_component_session_turn`. It accepts:

- an admitted `CompiledProtocolComponent`;
- `ProtocolSessionTurnBudgets`, containing independent framing and processing
  invocation budgets;
- the trusted `FrameProcessingGate`;
- the mutable host-owned `SessionState`; and
- the mutable host-owned `SessionInputBuffer`.

The operation first calls the existing host-integrated framing boundary. A
need-more-data decision ends the turn without invoking processing or changing
the buffer or session state. A complete frame is removed from the input buffer
and passed immediately to the existing host-gated processing boundary. Because
framing cannot mutate session state and both operations receive the same state
object, processing observes the state used to locate the frame.

Return `ProtocolSessionTurn::NeedMoreData` with the framing resource report when
input is incomplete. Return `ProtocolSessionTurn::Processed` after processing
and host validation succeed. Its `ProcessedProtocolSessionTurn` retains:

- the exact extracted `WireFrame`;
- accepted decoded outputs and staged replies;
- the framing invocation report; and
- the processing invocation report.

Classify failures as `ProtocolSessionTurnError`:

- `Framing` preserves the existing framing error and occurs before any bytes are
  removed;
- `Processing` retains the processing or host-validation error, the exact frame
  already removed from the input buffer, and the successful framing report.

Processing failure preserves the pre-turn session state and exposes no partial
decoded output or staged reply. The extracted frame remains consumed from the
session input buffer and is transferred into the error. Reinsertion is not part
of this operation because it would blur ownership, complicate ordering, and
permit an automatic retry policy that the session host has not selected.

Each call handles at most one frame. Callers schedule another turn to process a
coalesced following frame.

## Consequences

- Session integrations have one safe operation for the normal framing and
  processing path.
- Incomplete input consumes only a framing budget and causes no side effects.
- Framing and processing resource use remain distinct for metrics, limits, and
  package-health decisions.
- A successful turn retains the raw frame needed by later durable-acceptance and
  provenance stages.
- Processing failures cannot lose the extracted frame, even though it no longer
  resides in the connection buffer.
- Host validation remains the only path by which component state changes,
  decoded outputs, and replies become visible.
- One connection cannot drain all coalesced frames in a single unbounded turn.

## Alternatives considered

### Let every transport session compose the two operations

This duplicates subtle ownership and failure behavior. A caller could discard
the frame after a processing failure, use unrelated state between operations,
or lose one of the invocation reports.

### Use one shared invocation budget

Framing and processing execute in separate component instances and have
different operational frequency. One value would obscure which stage exhausted
its allowance and make production tuning less precise.

### Put a failed frame back into the input buffer

Automatic reinsertion selects a retry policy and risks repeated processing or
incorrect ordering with retained coalesced bytes. Returning ownership of the
frame lets a higher-level session policy make that decision explicitly.

### Process all complete buffered frames in one turn

An internal loop would combine multiple failure points and resource reports and
could let a busy connection monopolize a worker. One-frame turns preserve a
clear scheduling boundary.

### Merge framing and processing into one component export

Incomplete-input retries could then repeat proposed state changes, outputs, or
replies. The two-stage WIT contract intentionally keeps framing side-effect-free.

## Out of scope

- transport reads, socket ownership, and connection scheduling;
- automatic retry, quarantine, or connection-closure policy;
- durable raw-frame acceptance and accepted-message construction;
- decoded-observation assembly and persistence;
- releasing durability-gated replies or writing replies to transport;
- looping over coalesced frames;
- package selection, activation, upgrades, and rollback;
- production invocation budgets and tenant-specific overrides.
