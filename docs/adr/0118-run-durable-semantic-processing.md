# ADR 0118: Run durable semantic processing

## Status

Proposed, 27 October 2026.

Supersedes: none. Superseded by: none.

## Context

Protocol replay commits decoded facts without attributing business meaning. The
existing resolver and history processor handle one observation, but a process
crash must not forget unresolved work or select corrected metadata during an
ambiguous history append. An ingestion checkpoint is not semantic completion.

## Decision

History owns a narrow scheduling port and a one-claim orchestration use case.
PostgreSQL implements the port; the server composes a checked-schema worker.
The server acquires one new inward dependency on history. No domain or policy
crate acquires storage, transport or runtime dependencies.

Migration 0035 installs a version-1 decoded-output handoff: an AFTER INSERT
trigger creates history-owned work in the source transaction. This is an
explicit cross-capability publication contract, not semantic logic in the
protocol decoder. A singleton, locked keyset cursor discovers at most 32 older
outputs per turn; new inserts behind its cursor are covered by the trigger.
There is no migration-time backfill or scan of already completed work.
Initial and continuing discovery use separate lateral paths so the continuing
key comparison remains an indexable range, rather than an OR join filter.

Policy 1 selects preserved device time if present, otherwise host receipt time.
That basis and a database-clock inclusive knowledge cutoff are saved once at
first enqueue. Device time is an attribution choice, not an accuracy claim;
future timestamp authority policies require a new recorded policy version.

Claims last 30 seconds and have at most three monotonically fenced generations.
Selection of an exact binding revision commits before append. Recovery loads
that immutable revision and exact property definition, rather than discovering
new binding candidates. History append and terminal completion then share one
transaction. A lost commit acknowledgement cannot duplicate an interpretation.
Unresolved configuration stores a closed category and retains the exact source
and time choices. Exhausted work also remains durable; neither outcome is
automatically reinterpreted when metadata changes.

Before each decision write or completion, lock the owning tenant FOR SHARE,
then lock and validate the work generation/expiry. Suspension therefore orders
against these transactions: completion linearizes before suspension or is
denied after it. Claims can race with suspension but authorize no append by
themselves. All source and binding references include tenant ownership.

The runtime checks the exact migrated schema, uses four database connections,
one in-flight claim, five-second statement deadlines and a ten-second turn
deadline. Cancellation rolls back transactions. Routine errors expose only a
bounded category; raw data, SQL errors and connection strings stay out of logs.

## Consequences

Scheduling state is mutable but original meaning, pinned selection and terminal
evidence are protected by constraints and a trigger. Workers recover without
an external queue. Long outages can retain exhausted work requiring later
explicit operator tooling; this PR does not expose reset or reinterpretation.
Already published history remains immutable. A backdated metadata insertion
that invalidates a saved interpretation fails closed at lineage validation;
it never authorizes selection of a different interpretation on retry.

This establishes functional processing, not a throughput SLA, hardware clock
qualification, history HTTP API, live projection or deployment qualification.
D06 and full-profile qualification remain deferred. Operations must migrate
before starting the worker; direct database access remains privileged, with
the same role assumptions as existing ingestion/history adapters.

## Alternatives considered

- Resolve on every retry: rejected because a crash could silently change the
  selected binding or change an unresolved result into success.
- Append then independently acknowledge work: rejected because it complicates
  suspension fencing and leaves avoidable ambiguous progress.
- A broker or separate service: deferred; PostgreSQL already owns immutable
  facts, claims and transaction boundaries.
- One migration-time historical backfill: rejected because its cost grows with
  retained source history and delays deployment.

## Verification

Real PostgreSQL tests cover committed/rolled-back handoff, bounded historical
seeding with real EXPLAIN positive/negative indexability controls, explicit
device/receipt bases, competing claims, stale generations,
restart with correction, uncertain acknowledgement, atomic append failure,
suspension/reactivation, unresolved retention, exhaustion and immutable choices.
The architecture sensor checks the composition-root dependency; process parser
tests cover the explicit mode and rejected extra arguments.
