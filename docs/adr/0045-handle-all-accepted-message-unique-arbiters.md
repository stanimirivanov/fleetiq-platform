# ADR 0045: Handle all accepted-message unique arbiters

- Status: accepted
- Date: 2026-09-20
- Supersedes: [ADR 0031](0031-resolve-accepted-message-appends-with-a-fresh-snapshot.md)

## Context

[ADR 0031](0031-resolve-accepted-message-appends-with-a-fresh-snapshot.md)
established a two-statement append algorithm for accepted messages. Its insert
used `ON CONFLICT (tenant_id, message_id) DO NOTHING`; a second statement then
read the immutable winner using a fresh `READ COMMITTED` snapshot and compared
all persisted content.

The decoded-observation schema later added
`uq_accepted_messages_decoder_reference` across tenant, message, protocol
package, and package version. PostgreSQL requires that uniqueness guarantee so
a decoded-result foreign key can prove it names the exact package release
pinned to the accepted message.

An accepted message therefore has two unique indexes. During concurrent
identical inserts, PostgreSQL may encounter the wider decoder-reference index
instead of the primary-key arbiter selected by the targeted conflict clause.
That secondary violation is not suppressed and escapes as a repository
conflict, even though the competing row is an exact idempotent retry. The
concurrency test exposed this behavior intermittently in local and CI runs.

Every current uniqueness constraint on `ingestion.accepted_messages` contains
the same `(tenant_id, message_id)` identity. A conflict on either constraint
therefore has one immutable row that the existing primary-key fallback read can
resolve.

## Decision

Retain the two-statement fresh-snapshot algorithm and complete-content
comparison from ADR 0031, but insert accepted messages with unqualified `ON
CONFLICT DO NOTHING`.

The unqualified clause suppresses conflicts from both the primary key and the
wider decoder-reference uniqueness constraint. If no row is returned, the
repository reads by `(tenant_id, message_id)` in a new statement, reconstructs
the domain value, and compares every immutable field with the candidate. Exact
content returns `AlreadyPresent`; different content returns
`IdentityCollision`.

Foreign-key, check, and other non-uniqueness failures remain errors. This choice
depends on every suppressible accepted-message uniqueness constraint resolving
to the same tenant-scoped message identity. A future unique constraint with
different conflict meaning must either preserve that property or replace this
algorithm through another decision.

No schema or persisted-data migration is required.

## Consequences

- Concurrent exact appends cannot leak either accepted-message uniqueness
  constraint as an operational conflict.
- The normal first append still uses one statement, while retries and identity
  collisions use the existing fresh-snapshot read.
- Complete comparison continues to distinguish an exact retry from reuse of an
  identity with different device, package, time, or payload content.
- The decoder-result foreign key retains its exact-package guarantee.
- Repository documentation and SQLx query metadata must describe the
  unqualified conflict clause.
- Adding another unique constraint to the table requires reviewing whether all
  of its conflicts can use the same identity-resolution path.

## Alternatives considered

### Keep the primary-key conflict target

This leaves concurrent inserts able to fail on the decoder-reference unique
constraint and violates the idempotent append contract.

### Catch unique violations in Rust and then read the winner

This can recover from the secondary constraint, but duplicates the database's
conflict handling across successful and error paths. Unqualified `DO NOTHING`
expresses the intended behavior directly while leaving non-unique failures
unchanged.

### Remove the decoder-reference uniqueness constraint

PostgreSQL needs a matching uniqueness guarantee for the decoded-result foreign
key. Removing it would weaken exact package provenance or require redesigning
that persisted relationship.

### Use a no-op update to return the winner

An idempotent retry would become a write, create row-version churn, and conflict
with append-only update protection.

## Out of scope

- changing accepted-message identity or immutable fields;
- changing decoded-result foreign keys;
- batch append or copy-based ingestion;
- retry timing, backoff, or circuit breaking;
- retention, partitioning, and archival; and
- row-level security and database role design.
