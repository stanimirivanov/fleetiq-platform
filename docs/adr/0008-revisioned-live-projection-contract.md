# ADR 0008: Revisioned live projection contract

- Status: accepted
- Date: 2026-09-11

## Context

FleetIQ dashboards will combine an initial state snapshot with ongoing changes.
Reading a snapshot and then opening an unrelated subscription can lose changes
committed between those operations. Slow consumers can also exhaust memory if a
server retains an unlimited queue, while silently dropping changes would leave
the client with plausible but stale state.

Authorization can change while a client is connected. Continuing an old stream
may retain data the client should no longer see, and applying new-scope deltas to
old-scope state can produce an invalid mixed projection.

The contract must express continuity and recovery independently of REST,
WebSocket, server-sent events, a broker, or an eventual serialization format.
Different projections may use different payloads and revision sources.

## Decision

Add generic revisioned snapshot and live-update contracts to the application
crate.

`ProjectionRevision` is an unsigned, monotonic continuity token scoped to one
authorized projection. Revision zero is a valid initial state. Incrementing a
revision is checked and never wraps at the numeric limit. Revisions from
different projections are not comparable.

A `ProjectionSnapshot<T>` contains the complete authorized collection at one
revision. A `ProjectionDelta<T>` contains ordered changes, a required base
revision, and a resulting revision. The resulting revision must be greater than
the base. A delta may span several internal commits, allowing bounded
coalescing, provided it represents the complete transition between those
revisions.

A `RevisionCursor` starts at a snapshot or retained revision. It advances only
when a delta's base exactly equals its current value. A mismatch returns a
structured error and leaves the cursor unchanged. Consumers advance the cursor
only after applying the delta changes successfully.

The server must establish a gap-free snapshot and stream handoff. It may:

- establish the subscription before reading the snapshot, buffer a bounded
  sequence, and discard buffered changes at or before the snapshot revision; or
- read a committed snapshot and replay a durable change log or outbox strictly
  after that revision.

The first usable delta must have the snapshot revision as its base. This
contract records the required behavior without selecting an implementation.

`LiveMessage<T>` carries either a delta or `ResnapshotRequired`. Resnapshot
reasons distinguish:

- a consumer that lagged beyond a bounded queue or retained change history;
- a revision mismatch;
- an authorization change;
- a projection revision sequence that was reset.

After a resnapshot instruction, the consumer discards its local projection and
obtains a complete snapshot. Authorization changes stop delivery from the old
visibility scope before a new authorized snapshot is created.

## Consequences

- Clients can detect missing, duplicated, stale, or out-of-order delta delivery
  through an exact base-revision comparison.
- Coalesced deltas can reduce delivery overhead without hiding continuity.
- Cursor state does not advance when validation fails.
- Slow consumers recover through a bounded resnapshot path instead of creating
  unbounded server queues.
- Authorization changes have an explicit projection-rebuild path.
- The same contract can carry asset state, maps, diagnostics, alerts, or other
  projection-specific change types.
- Implementations must provide a consistency barrier or durable replay source
  for the snapshot-to-stream handoff.
- Queue limits, retention duration, resume tokens, payload schemas, transport,
  serialization, and authorization enforcement remain later decisions.

## Alternatives considered

### Send live values without revisions

Clients cannot distinguish a current stream from one with silently lost or
reordered messages. Reconnecting also has no reliable resume point.

### Put only the resulting revision on each delta

The client cannot determine whether it holds the state required to apply that
delta. Including the base revision makes continuity explicit.

### Require every delta to increment by exactly one

This prevents safe coalescing and exposes internal commit granularity to public
delivery. Exact base matching provides continuity while allowing a complete
multi-revision transition.

### Buffer every missed update for every connected client

An unbounded queue lets a slow or disconnected consumer exhaust server memory.
Bounded delivery with explicit resnapshot recovery has a predictable resource
limit.

### Reopen a stream after returning an independently read snapshot

A change committed between the snapshot read and subscription can be lost. The
implementation needs a shared consistency barrier or replayable change source.

### Continue the existing stream after authorization changes

Previously visible state may no longer be authorized, and delta semantics may
depend on a different result set. Rebuilding from a fresh authorized snapshot
avoids mixed visibility scopes.
