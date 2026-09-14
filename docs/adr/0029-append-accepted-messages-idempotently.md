# ADR 0029: Append accepted messages idempotently

- Status: accepted
- Date: 2026-09-14

## Context

A successful protocol session turn produces an exact wire frame, validated
decoded outputs, and staged replies. FleetIQ must durably retain that frame
before it creates trusted observations or releases a reply whose protocol
meaning promises data acceptance. The existing `AcceptedMessage` type describes
the fact after durability, but no application port marks where that transition
occurs.

Storage technology remains undecided. PostgreSQL, an append-only event log, or a
broker-backed durable stream can satisfy different operational requirements.
The application contract should state the required behavior without exposing a
database transaction, broker acknowledgement, offset, or object-storage key.

Append results can be ambiguous. A process may lose its response after storage
commits, and a reconnect or worker retry must not create another logical
message. Conversely, silently accepting the same identifier with different
tenant, device, package, time, or payload would corrupt provenance.

## Decision

Add `PendingAcceptedMessage` to the application crate. It carries the complete
trusted append candidate:

- a stable host-assigned accepted-message identity;
- admitted tenant and communication-device identity;
- the immutable protocol-package release pinned to the session;
- the host receipt time for the complete frame; and
- the exact opaque wire payload.

The pending type is distinct from telemetry's `AcceptedMessage`. It indicates
that the external durability condition has not yet succeeded. Its `Debug`
representation delegates payload formatting to `AcceptedPayload`, which reports
only length and does not expose wire bytes.

Add the consumer-owned `AcceptedMessageLog` port. Its asynchronous `append`
operation borrows the candidate so a caller retains the exact value for retry
after an ambiguous error. The implementation-specific error remains associated
with the port.

An implementation may return success only after its configured durability
condition has completed. `AcceptedMessageAppendOutcome` distinguishes:

- `Appended`, when this call created the durable fact;
- `AlreadyPresent`, when an identical candidate with the same identity was
  already durable.

Both outcomes carry the accepted message. Reusing an identity with any different
immutable field is a collision and must return an error. An error never permits
observation assembly or release of durability-gated replies. The caller retries
an ambiguous failure with the same identity and unchanged candidate.

Keep the port independent of a concrete durable log. A later adapter must
document its precise commit and acknowledgement condition and enforce the
content-equality rule atomically.

## Consequences

- The application has an explicit transition from a complete framed candidate
  to a durably accepted message.
- Accepted-message identity is assigned before append and supports safe retries.
- Duplicate delivery can be observed separately from a first append without
  changing logical message identity.
- Identity collisions cannot silently rewrite admission or decoder provenance.
- The candidate remains available after storage errors, including errors whose
  commit outcome is unknown.
- Protocol processing, storage technology, observation persistence, and
  transport output remain separate responsibilities.
- Concrete adapters must compare every immutable field when resolving a
  duplicate identity.

## Alternatives considered

### Append `AcceptedMessage` directly

That name represents a completed durability transition. Passing it into the
operation that establishes durability makes pre- and post-commit states harder
to distinguish in application code.

### Assign the message identity after append

A caller could not safely retry an ambiguous failure without creating another
logical message. Preassignment gives every attempt the same idempotency key.

### Treat every duplicate identity as success

This would hide collisions where one identity refers to different payload or
provenance. Only an exact immutable-content match is an idempotent retry.

### Return no append disposition

Both first append and idempotent retry produce an accepted message, but their
distinction is useful for metrics and operational diagnosis. An explicit
outcome preserves it without changing downstream semantics.

### Select PostgreSQL or a broker in this contract

The application requires durable, idempotent append behavior rather than a
specific storage API. Adapter selection needs separate evaluation of throughput,
retention, replay, availability, and cost.

## Out of scope

- selecting or implementing the durable storage adapter;
- SQL schema and migrations for accepted messages;
- generating accepted-message identities;
- proving that a caller waited for durability before converting the candidate;
- composing a session turn with the append operation;
- assembling decoded observations after acceptance;
- persisting decoded observations;
- releasing deferred replies or writing replies to transport;
- retention, archival, encryption, and payload-reference policies;
- production retry timing, backoff, and circuit breaking.
