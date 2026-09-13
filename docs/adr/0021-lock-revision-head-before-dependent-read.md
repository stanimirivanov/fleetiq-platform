# ADR 0021: Lock a revision head before reading its dependent row

- Status: accepted
- Date: 2026-09-13

## Context

Asset relationships and signal bindings use the same append-only persistence
pattern. A mutable identity row stores the latest revision number, while an
immutable revision table stores the complete fact at each revision. Appending a
revision locks the identity row so writers for one aggregate serialize without
blocking unrelated aggregates.

The original repository query joined the mutable identity to its current
immutable revision and used `FOR UPDATE OF identity`. Under PostgreSQL's default
`READ COMMITTED` isolation, the statement takes its snapshot before it waits for
a concurrent writer. PostgreSQL can return the updated version of the locked
identity row after the wait, while other rows read by that statement still come
from its earlier snapshot. The join can consequently stop matching or retain
stale dependent data after the winner advances the head.

This made the concurrent-append tests nondeterministic. Exactly one revision-2
append still succeeded, but the loser could reach a database `RowNotFound` or
uniqueness failure instead of observing latest revision 2 and returning the
domain-facing `UnexpectedRevision { expected: 3, actual: 2 }` error.

PostgreSQL documents both parts of this behavior: a locking statement returns
the updated version of a row after waiting, and each subsequent command at
`READ COMMITTED` receives a new snapshot containing transactions committed
before that command starts. See the PostgreSQL
[`READ COMMITTED` documentation](https://www.postgresql.org/docs/18/transaction-iso.html#XACT-READ-COMMITTED)
and [`SELECT` locking-clause documentation](https://www.postgresql.org/docs/18/sql-select.html#SQL-FOR-UPDATE-SHARE).

## Decision

For both relationship and signal-binding appends, split existing-head lookup
into two statements inside the same transaction:

1. select only the mutable identity's `latest_revision` using `FOR UPDATE`;
2. parse the locked revision and validate that the proposed revision immediately
   follows it;
3. if the sequence is valid, select the corresponding immutable revision's
   `recorded_at_ms` in a separate statement;
4. validate recorded-time ordering, insert the new immutable revision, advance
   the locked head, and commit.

The second statement begins after the identity lock has been acquired. At
`READ COMMITTED`, it therefore receives a fresh snapshot that includes the
concurrent winner's committed revision.

Return `UnexpectedRevision` immediately after reading the locked head when a
waiting append is now stale. It does not need to read the prior recorded time.

Retain separate implementations in each SQLx repository. Their table and column
names differ, while extracting a generic locking framework would obscure the
queries and weaken compile-time checked SQL without removing meaningful domain
duplication.

Improve both concurrency-test assertions with the complete pair of repository
results so any future classification failure reports the actual database or
domain errors.

## Consequences

- Concurrent appends to one relationship or signal binding still serialize on
  its stable identity row.
- The losing duplicate revision observes the committed winner through the
  locked head and returns the expected typed sequence error.
- Related immutable revision data never depends on a snapshot taken before a
  lock wait.
- A valid append to an existing identity performs one additional database round
  trip for recorded-time lookup.
- A stale append returns after the locked-head query and does not perform that
  additional lookup.
- Unrelated aggregate identities remain independently lockable.
- The behavior assumes PostgreSQL `READ COMMITTED`, which is the database
  default and the isolation level used by the repository integration tests.
  Deployments using `REPEATABLE READ` or `SERIALIZABLE` must handle PostgreSQL
  serialization failures as a separate retry policy.

## Alternatives considered

### Accept either a typed sequence error or a database conflict in the test

That would hide unstable repository behavior from callers. The loser is stale
after waiting for the winner and the repository has enough information to
report the domain-facing sequence error consistently.

### Keep the joined locking query

Locking only the identity table does not refresh other rows participating in the
same statement. The join is precisely what makes the statement vulnerable to a
mixed post-wait view.

### Retry the joined query after `RowNotFound`

This treats one observed symptom without preventing a uniqueness conflict or
stale recorded-time comparison. Separating the lock from dependent reads makes
the snapshot boundary explicit.

### Use a higher transaction isolation level

`REPEATABLE READ` and `SERIALIZABLE` can abort the waiting transaction with a
serialization failure. They still require a retry policy and would change the
behavior and cost of every repository transaction.

### Replace the flow with an optimistic compare-and-swap update

An atomic conditional head update can serialize the sequence number, but
failure still needs a subsequent read to produce the expected revision and the
repository must validate recorded-time ordering before committing the immutable
row. The explicit row lock keeps the existing append model clear.

### Generalize both repositories behind one revision framework

The similar concurrency protocol is worth documenting, but the aggregates have
different schemas, validation, reconstruction, and queries. A generic framework
would add indirection before enough stable shared behavior exists.

## Out of scope

- retry policy for serialization failures or deadlocks;
- changing the transaction isolation level;
- lock timeouts and statement timeouts;
- optimistic concurrency tokens exposed through application APIs;
- batching revisions for one aggregate;
- changing deferred database constraints or immutable-history triggers;
- performance tuning based on production append measurements;
- repository patterns for aggregates without revision heads.
