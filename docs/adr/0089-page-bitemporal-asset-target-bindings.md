# ADR 0089: Page bitemporal bindings targeting an asset

## Status

Accepted, 2 October 2026.

Supersedes: none. Superseded by: none.

## Context

The protected source-binding snapshot explains where one decoded signal is
attributed, but an asset dashboard needs the reverse read: which recorded
bindings can populate this asset's properties at a specified physical and
recorded time. An asset can have many bindings, and corrections can move a
binding between target assets. A flat scan of all tenant bindings or filtering
older matching revisions would make this read costly or historically wrong.

## Decision

`GET /api/v1/tenants/{tenant_id}/assets/{asset_id}/signal-bindings` requires
signed Unix-millisecond `effective_at_ms` and `known_at_ms` query parameters.
`limit` defaults to 50 and accepts 1 through 100. Optional `after` is an
exclusive stable binding-ID cursor. The response contains exact binding
revision projections ordered by binding ID and `next_after` only when another
row exists. Current tenant asset existence distinguishes `404` from an
existing asset with no effective bindings (`200` empty); existence is not
historical.

The adapter discovers candidate identities that targeted the asset by the
recorded cutoff, selects each latest known revision, then filters its current
target and half-open effective interval. PostgreSQL returns at most
`limit + 1` rows. A tenant-and-target-leading index supports candidate
discovery without changing binding meaning or write invariants. The service
derives tenant scope only from consumed `ReadMetadata` evidence. The route
reuses configured bearer authentication, a fresh exact-tenant `metadata.read`
grant, generic protected errors, and `Cache-Control: no-store`.

## Consequences

Clients can explain an asset's configured signal attribution without assuming
an industry-specific schema or materialized property state. They must repeat
both time cutoffs and the cursor across pages. The recorded cutoff filters
data; it does not freeze commits across requests. A backdated correction can
change later pages, so a client needing a consistent traversal must restart.

The page limit bounds returned revisions but candidate discovery can still
examine many historical identities for a high-degree asset. Measure query
plans and latency before claiming large-asset capacity. The ordinary index
build takes a table lock; this pre-release migration is appropriate for the
current empty/development database, while a future deployed large table needs
a separately planned online index rollout.

## Alternatives considered

An unbounded reverse lookup was rejected because assets can accrue many
bindings. Filtering target revisions before selecting the latest known
revision was rejected because it can resurrect a binding moved away by a
correction. A `404` for an existing unbound asset was rejected because the
asset exists independently of attribution configuration.
