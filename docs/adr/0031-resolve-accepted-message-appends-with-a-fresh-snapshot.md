# ADR 0031: Resolve accepted-message appends with a fresh snapshot

- Status: accepted
- Date: 2026-09-14

## Context

The accepted-message log requires three outcomes under concurrency: one caller
durably inserts a new message, an exact retry returns the existing accepted
message, and reuse of the same tenant-scoped identity with different immutable
content returns a collision. The PostgreSQL schema supplies an immutable row and
composite primary key, but a key conflict alone cannot distinguish the latter
two cases.

A tempting implementation uses one data-modifying common table expression:
insert with `ON CONFLICT DO NOTHING`, then select the existing row if the insert
returned nothing. Under PostgreSQL `READ COMMITTED`, all parts of that statement
share one snapshot. The unique-index conflict check can wait for and observe a
concurrent uncommitted row, while the fallback `SELECT` still cannot see that
row in the statement's older snapshot. The statement can therefore return no
row even though another transaction just committed the winning message.

Using `ON CONFLICT DO UPDATE ... RETURNING` would make the winner visible in one
statement, but it performs a write for an idempotent read, creates extra row
versions, fires triggers, and conflicts with the table's append-only update
prohibition.

## Decision

Implement `PostgresAcceptedMessageLog` with two conditional statements:

1. Execute `INSERT ... ON CONFLICT (tenant_id, message_id) DO NOTHING RETURNING
   TRUE`.
2. If the insert returns no row, issue a primary-key `SELECT` as a new statement.

The insert is the durability boundary for a new message. PostgreSQL waits for a
concurrent transaction that owns the same unique key before deciding whether
the insert succeeds or encounters a conflict. After conflict, the second
statement receives a fresh `READ COMMITTED` snapshot and can see the committed
winner.

No explicit transaction spans the two statements. The accepted-message table
rejects `UPDATE` and `DELETE`, so the selected row cannot disappear or change
after the primary-key conflict. Autocommit completion of the successful insert
establishes durability before the adapter returns `Appended`.

Reconstruct the selected row through telemetry domain constructors. Compare the
complete `AcceptedMessage` with the original candidate, including tenant,
message ID, device, package ID, package version, receipt time, and exact payload.
Return `AlreadyPresent` only for complete equality. Return the typed
`IdentityCollision` error when any immutable field differs.

Keep ordinary PostgreSQL errors behind `PostgresRepositoryError` and wrap them
in `PostgresAcceptedMessageLogError`. Invalid device references retain the named
foreign-key classification. The collision variant carries only safe tenant and
message identifiers and does not expose wire content.

## Consequences

- Concurrent exact appends have one `Appended` result and idempotent callers can
  observe `AlreadyPresent` reliably.
- The conflict path costs a second database round trip; the normal first-append
  path uses one statement.
- Exact retry checks every immutable field instead of trusting key equality.
- No fake update, trigger bypass, or row-version churn is required.
- Persisted values are reconstructed and validated before crossing the adapter
  boundary.
- Collision diagnostics identify the tenant-scoped message without logging its
  payload.
- The implementation relies on schema-enforced accepted-message immutability.

## Alternatives considered

### Use one data-modifying common table expression

Its fallback read can miss a row that was uncommitted when the statement
snapshot was taken but became the unique-conflict winner while the insert
waited. Returning no outcome would violate the port contract under concurrency.

### Use `ON CONFLICT DO UPDATE ... RETURNING`

This forces visibility but turns an idempotent retry into a database mutation,
creates row churn, fires update behavior, and violates the append-only schema.

### Start an explicit transaction and repeat the read

The table is immutable, and each statement already receives the required
`READ COMMITTED` visibility. A longer transaction adds lifecycle and failure
paths without strengthening the result.

### Treat any primary-key conflict as `AlreadyPresent`

The same identity could then silently name a different device, package release,
receipt time, or payload. That would corrupt replay and observation provenance.

### Compare only a payload digest

No digest is stored, and equality covers more than payload bytes. Comparing the
bounded stored row directly is exact and avoids introducing hash collision and
canonicalization policy.

## Out of scope

- accepted-message read and replay query ports;
- batch append and copy-based ingestion;
- generating accepted-message identities;
- retry timing, backoff, and circuit breaking;
- decoded-observation assembly and persistence;
- releasing durability-gated replies;
- connection-session orchestration;
- retention, partitioning, archival, and payload tiering;
- row-level security and database role design;
- performance indexes for undefined read workloads.
