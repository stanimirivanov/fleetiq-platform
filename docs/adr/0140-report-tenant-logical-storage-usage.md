# ADR 0140: Report tenant logical storage usage

## Status

Proposed, 6 November 2026.

## Context

M06 needs tenant cost evidence before archive and retention consumers exist.
Physical tables, indexes and free space cannot be attributed by summing tenant rows.
Property history references observations rather than copying their values.

## Decision

Expose GET /api/v1/tenants/{tenant_id}/storage-usage under current independent
operations.read, through the existing browser/workload authentication boundary.
Baseline.29 is additive and introduces no action names. Responses are no-store.

Reconcile authoritative tenant rows in a repeatable-read, read-only transaction.
Each measured class includes exact decimal-string record and byte totals. Accepted
messages count wire payload octets only. Decoded observations and property
interpretations count UTF-8 PostgreSQL row JSON serialization, including row
identifiers/context. Their method is named postgres_row_json_utf8_v1. These methods
are intentionally different; consumers must not infer total physical allocation
or billable storage. Schema changes affecting row serialization require review of
the named accounting method. Shared physical, archive and export usage are explicitly
not measured, never zero. No total across methods is advertised.

A two-second SQL statement deadline bounds reconciliation; the HTTP owner also
bounds execution to five seconds. Timeout, unavailable storage or invalid totals
fail closed. The database transaction timestamp labels the observation, not a
background refresh or long-lived cache. No cached counters can drift: each request
reconciles directly. Exact parsing rejects overflow and inconsistent zero-count
nonzero-byte pairs rather than returning rounded or fabricated evidence.

## Consequences

No migration, background process or dependency is added. Large tenants may exceed
the deadline; this is explicit unavailable evidence, not an empty report. Later
accounting projections require their own completeness/reconciliation contract.
Only three supported logical classes are measured; artifacts, indexes, metadata,
calculator evidence and database overhead are not covered by this report.

## Alternatives considered

- Sum relation sizes per tenant: rejected because allocation and indexes are shared.
- Cached mutable counters: deferred until measured reconciliation cost warrants them.
- Return zero for future archives: rejected because absence of measurement is not zero.

## Verification

Real PostgreSQL HTTP tests compare totals with independently read fixture rows,
verify separate current authority, cross-tenant isolation and revocation, and hold
a table lock to prove deadline failure cannot look like successful empty usage.
Domain tests preserve values beyond u64 and reject overflow/inconsistent totals.
OpenAPI validation, SQLx freshness, full Rust acceptance and supply-chain checks gate delivery.
