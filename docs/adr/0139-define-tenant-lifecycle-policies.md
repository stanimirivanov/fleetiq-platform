# ADR 0139: Define tenant lifecycle policies

## Status

Proposed, 6 November 2026.

## Context

M06 needs explicit retention and work-budget contracts before archive selection,
accounting and job admission. Existing immutable evidence is never implicitly expired.

## Decision

History owns validated lifecycle settings. Missing settings and explicit disabled
revisions preserve current behavior. Configured settings require positive hot and
archive durations in days, concurrent jobs and reserved bytes; archive duration
must be at least hot duration. These are operator inputs, not recommended defaults.
Archive duration does not authorize permanent object deletion.

PostgreSQL stores an append-only tenant/revision ledger with actor and database
recording time. A tenant row lock serializes expected-revision appends; stale writes
fail rather than overwrite policy. The trusted persistence adapter is not an HTTP
administration service and does not grant authorization. Later public commands must
obtain exact authority before calling it. No new action vocabulary is introduced.

Protection reasons distinguish lineage anchors, idempotency records, active
replay/semantic/calculator/restore consumers and unverified archives. Only verified,
unreferenced payload is a candidate; classification is not execution authority.
Future selectors must combine all protections and recheck them transactionally.
No selector, object I/O, expiration, admission enforcement or deletion is shipped.

## Consequences

Migration 0052 creates an empty ledger without rewriting existing data. It takes
ordinary schema locks. Deploy schema before policy writers. Reverting serving code
leaves ledger evidence intact; do not drop or downgrade the migration. Foreign keys
retain referenced tenant/actor identities. No operational duration or provider is
chosen here; later consumers require reviewed workload limits.

## Alternatives considered

- Implicit retention defaults: rejected because workload and recovery limits are not yet qualified.
- Mutable policy rows: rejected because they lose revision and actor evidence.
- Generic deletion permission: rejected because protected consumers and lineage need independent checks.

## Verification

Constructor tests cover invalid limits and disabled defaults. Real PostgreSQL tests
cover tenant isolation, concurrent successor conflicts, attribution, disabling,
foreign keys, direct invalid SQL and immutable revisions. Full Rust acceptance and
supply-chain gates remain required before delivery.
