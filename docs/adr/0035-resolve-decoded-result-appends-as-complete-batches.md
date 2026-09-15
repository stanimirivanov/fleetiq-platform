# ADR 0035: Resolve decoded result appends as complete batches

- Status: accepted
- Date: 2026-09-15

## Context

The application now owns a `DecodedObservationLog` port, and PostgreSQL stores
one decoded-message result marker with zero or more ordered observation rows.
The adapter must implement the port's atomicity and exact-idempotency semantics
under ordinary retries, ambiguous failures, and concurrent writers.

A device may produce hundreds or thousands of observations in one message.
Issuing one insert per observation would turn a batch into `1 + N` database
round trips and lengthen the transaction. The storage representation uses a
separate marker, so a failed child insert must never leave visible completion
evidence or an observation prefix.

`AcceptedMessage` carries the trusted parent content used to assemble the
batch, but Rust construction alone cannot prove that this exact value is the
one stored in PostgreSQL. A foreign key over tenant, message, and package proves
identity and decoder release; it does not compare device, receipt time, or raw
payload. Persisting output assembled against different parent content would
silently attribute observations to the stored message.

As with accepted-message append, `INSERT ... ON CONFLICT DO NOTHING` may wait
for another transaction. A read in a later `READ COMMITTED` statement receives
a fresh snapshot and can see the immutable winner. A fallback read inside the
same data-modifying common table expression could retain the earlier snapshot
and miss that row.

## Decision

Implement `DecodedObservationLog` with `PostgresDecodedObservationLog`.

Open one transaction for every append. Insert the result marker with
`INSERT ... SELECT` from `ingestion.accepted_messages`, matching every immutable
accepted-message field:

- tenant and message identity;
- protocol-package identity and version;
- admitted communication-device identity;
- host receipt time; and
- exact wire payload.

The insert therefore succeeds only when the caller-held parent is identical to
durable acceptance evidence. It performs this check as part of the marker
statement rather than adding a preflight round trip.

When the marker is new, encode observation fields into parallel typed arrays
and insert all children with one `UNNEST` statement. Empty results skip the
child statement while retaining their marker. Commit only after every child row
succeeds. Any statement or commit failure returns an error and exposes no
successful partial batch.

When the marker insert returns no row, issue later statements in the same
`READ COMMITTED` transaction to read the immutable result and its accepted
message, followed by all child observations ordered by output index. These
statements use fresh snapshots after any conflict wait.

Classify absence of an exact accepted message, including different device,
receipt time, or payload under the same identity, as
`AcceptedMessageMismatch`. This remains separate from decoded-result collision.

Validate that the stored marker count equals the number of child rows. A
disagreement is corrupt persisted data. Reconstruct every row through domain
constructors, including identifiers, typed values, units, quality, device time,
and deterministic output identity. Compare the reconstructed complete batch to
the candidate:

- an exact match returns `AlreadyPresent`;
- different count, order, source, time, value, unit, or quality returns
  `IdentityCollision`.

The adapter's collision errors contain stable tenant, message, and package
identities without including raw payload bytes or decoded values.

Organize the adapter behind a small public facade. Keep transaction and batch
encoding in `append`, and keep private SQL row reconstruction in `row`, because
those responsibilities change for different reasons.

## Consequences

- A nonempty first append uses one marker statement and one set-based child
  statement inside a transaction, independent of observation count.
- Empty decoding uses only the marker statement and remains explicit durable
  completion.
- Failed child persistence cannot commit a marker or observation prefix.
- Exact concurrent appends produce one `Appended` and one `AlreadyPresent`
  outcome.
- Concurrent different appends produce one winner and one identity collision.
- Conflict resolution sees a concurrently committed winner through a fresh
  PostgreSQL snapshot.
- Derived observations cannot be persisted against different caller-held
  accepted-message content.
- Exact retries compare the complete stored result rather than marker identity
  or observation count alone.
- Decimal values are cloned into the SQLx array representation for batched
  encoding; this bounded in-memory cost avoids per-observation database trips.
- The implementation remains behind the application-owned port and exposes no
  SQLx rows or transaction types.

## Alternatives considered

### Insert each observation in a loop

This creates `1 + N` round trips and holds the transaction open across every
network exchange.

### Read the accepted message before inserting the marker

A separate preflight query adds a round trip. `INSERT ... SELECT` can require
the exact parent content in the marker statement itself.

### Trust the accepted-message composite foreign key

The key verifies tenant, message, and package identity but does not prove that
the caller's device, receipt time, and payload match the stored parent.

### Treat any result-marker conflict as an exact retry

The existing batch may differ in count, order, source metadata, timestamps,
values, units, or quality. Key equality is insufficient.

### Combine insert and fallback read in one data-modifying common table expression

After waiting for an uncommitted conflicting insert, the statement's original
snapshot may not see the row that caused the conflict. A later statement at
`READ COMMITTED` receives the required fresh snapshot.

### Compare only observation count or a digest

Count does not prove content equality. A digest adds encoding and collision
policy while the initial immutable rows can be compared exactly on the retry
path.

## Out of scope

- coordinating raw accepted-message append and decoded-result append in one
  transaction;
- automatic retries, backoff, dead-letter handling, and replay workers;
- decoded-observation query ports;
- semantic signal binding and asset-property change persistence;
- publication to live projections or external brokers;
- partitioning, retention, archival, and compression;
- performance indexes for undefined read paths; and
- database role and row-level security policy.
