# Calculator evaluation evidence

## TL;DR

The read-only APIs under
`/api/v1/tenants/{tenant_id}/assets/{asset_id}/calculator-evaluations` require fresh
`telemetry.read`, independent of metadata or analytics management. All responses
are no-store and bounded to 1 MiB. Baseline.23 publishes these routes.

## Reads

- `GET ?start_at_ms=0&end_at_ms=4000&limit=50` returns terminal summaries. Time
  parameters are canonical signed 64-bit decimal strings. The half-open range
  filters evaluation event time (window end), spans at most seven days, and has a
  maximum page size of 100. Empty scopes return empty pages.
- `GET /{evaluation_id}` returns the exact terminal summary and up to 1024 pinned
  input evidence IDs/times. It preserves the release/ABI, immutable execution
  configuration, input property version, original window/cutoff, first processing
  time, outcome and counts. It does not expand raw samples or outputs.
- `GET /{evaluation_id}/outputs?limit=20` returns ordered outputs, at most 20 per
  page. Values remain exact decimal text; target asset, property/version, unit and
  evaluation/release/zero-based ordinal identity are retained. Zero-output outcomes
  return empty output pages. Missing exact evaluations return 404.

Follow `next_after` using the same resource, tenant, asset and time bounds. Page
size may change. Tokens are opaque public positions, not credentials or snapshots.
Foreign/malformed cursors, invalid limits, unknown query fields and invalid time
ranges return 400. New backdated evaluations may fall before an earlier cursor.

Input references identify the original semantic selection. Existing property
history reads use the pinned scope/property/window; they may also contain later
facts. Do not substitute fresh history or calculator runs for the retained IDs.
This slice provides no raw preparation or summarized median/MAD explanation endpoint.

## Operations

Apply migration 0044 before deploying checked-schema readers. Its ordinary index
build scans existing evaluations and can block concurrent writers; use the existing
migration window. It changes no facts, grants, worker state or current-property API.
A rollback build must retain the migration inventory/index while removing route
exposure. See [ADR 0129](../adr/0129-read-bounded-calculator-evidence.md).
