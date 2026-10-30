# ADR 0129: Read bounded terminal calculator evidence

## Status

Proposed, 30 October 2026.

Supersedes: none. Superseded by: none.

## Context

Terminal envelopes retain exact evidence under [ADR 0124](0124-pin-calculator-execution-evidence.md).
Operators need protected reads without re-running calculators, expanding raw
preparation or multiplying large input/output arrays across a collection page.

## Decision

Use existing exact `telemetry.read` authority for tenant/semantic-asset reads.
Expose evaluation summaries over a half-open event-time range of at most seven
days, exact evaluation manifests and separate output pages for an exact evaluation.
Event time is window end; first processing time remains a separate retained field.

List pages return at most 100 summaries in ascending event-time/evaluation-ID C
order. Output pages return at most 20 entries in deterministic zero-based ordinal
order. Exact manifests retain at most 1024 semantic IDs/times, the input property
reference, immutable calculator release/ABI, execution configuration and window.
No decoded payload or prepared-value expansion is introduced. Existing history
reads provide semantic values separately; later facts must not replace selected IDs.

Reuse the existing bounded v1 hex cursor codec without changing its representation.
Bind positions by a public digest of resource, tenant, asset and list time bounds,
or exact output evaluation. Tokens are not signatures or authority. Reauthorize
every request; validate scope, position and limits before storage access. Pages
are not snapshots: later backdated appends can precede a previously issued cursor.

Exact times and decimal values remain strings. Output identity includes tenant,
evaluation, calculator release and ordinal; target/property/unit stay explicit.
Responses are no-store and capped at 1 MiB before sending, failing closed rather
than truncating evidence. Migration 0044 indexes the exact event/identity ordering.

## Consequences

No dependencies or grants change. Summaries never load input/output arrays into
HTTP pages; exact reads omit outputs, and output queries expand at most one bounded
envelope. Sharing the existing codec serves two current read capabilities; history
cursor fixtures guard representation compatibility. Empty collections return 200;
missing exact envelopes return 404. Unauthorized callers do not discover records.

Apply the additive index before deploying checked-schema readers. Roll back route
exposure in a forward build retaining the migration inventory and index. Facts,
workers and projection contracts do not change. Raw preparation, exports, long-range
analytics, numerical explanation endpoints and UI remain separate work.

## Alternatives considered

Returning whole envelopes in a list multiplies up to 1024 inputs/outputs per row.
Offset paging drifts across tied append-only event times and becomes increasingly
expensive. Signed cursors would not replace current authorization and are not needed
for public read positions. Re-evaluation would reinterpret retained evidence.
