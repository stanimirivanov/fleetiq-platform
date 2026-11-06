# ADR 0141: Enforce tenant history-work budgets

## Status

Proposed, 6 November 2026.

## Context

Bounded history pages and SQL deadlines do not prevent concurrent tenant requests
from oversubscribing configured lifecycle work slots and reserved bytes.

## Decision

A trusted PostgreSQL admission adapter serializes on the tenant row, the same lock
used by policy revision appends. It reconciles expired reservations, reads the latest
policy and atomically reserves one slot plus positive bytes, or returns an explicit
denial. Sums use NUMERIC to avoid signed BIGINT overflow. Each reservation pins its
policy revision and has a tenant-scoped, never-reused identity. Release cannot remove
another tenant's reservation. An absent/disabled policy retains unmetered behavior.

Protected history pages and exact provenance reads reserve 1 MiB of response bytes.
The HTTP exact-provenance path uses one joined, deadline-bounded query and the
existing validated row mapper; other observation readers retain their original ports.
Configured execution is bounded to five seconds, retains existing SQL deadlines of
at most five seconds, and checks serialized output against the allowance. This is
an output reservation, not a scan-byte or peak-memory guarantee. Existing 100-row,
seven-day limits remain. Budget denial or oversized output is no-store HTTP 429;
SQL/storage/timeout failures remain 500, never successful empty evidence. Authorization
and input validation precede admission. No new action or public configuration API.

Completed operations, including explicit failures, release the reservation. Dropped
or timed-out owners retain capacity because a database statement may still be
cancelling. Thirty-second database-clock expiry exceeds the combined execution and
SQL cancellation bounds. Subsequent admission removes expired rows transactionally;
no detached cleanup task or automatic retry is spawned. Restart therefore conservatively
holds capacity until expiry. Later job consumers must use bounded chunks with deadlines
inside this lease; this adapter does not license unbounded background work.

## Consequences

Migration 0053 adds a transient ledger without rewriting history. Deploy the schema
before serving the new code. Runtime roles need policy SELECT, reservation SELECT/INSERT/DELETE,
and tenant row-lock privileges; provision DML grants without DDL or owner credentials. Quiesce unmetered reads before first enabling or
re-enabling a configured policy: disabled work has no reservations. Lower limits
apply to new admissions; existing reservations remain charged until release/expiry.
Reverting serving code removes enforcement, so quiesce traffic before rollback;
retain the additive migration. Duration and byte constants are conservative implementation
bounds, not production capacity claims. Archive/export workers and ingress remain unchanged.

## Alternatives considered

- Process-local counters: rejected because replicas could oversubscribe independently.
- Immediate release on cancellation: rejected because SQL cancellation can lag the owner.
- Infinite leases: rejected because crashes would require manual capacity repair.
- Implicit policy defaults: rejected to preserve existing tenant behavior.

## Verification

Real PostgreSQL tests race slot and byte admission, test tenant-scoped release,
policy reductions, invalid bytes and deterministic expired-lease reconciliation.
HTTP tests exercise 429 versus successful history/provenance and zero residual slots.
Owned-future tests cover errors, oversized output and cancellation without sleeps.
Full Rust, OpenAPI and supply-chain gates remain required for delivery.
