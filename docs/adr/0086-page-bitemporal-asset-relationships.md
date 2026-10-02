# ADR 0086: Page bitemporal asset relationships through protected HTTP

## Status

Accepted, 2 October 2026.

Supersedes: none. Superseded by: none.

## Context

The metadata capability already reconstructs a relationship snapshot at
explicit physical and recorded times, but its collection read is unbounded
and unavailable to HTTP clients. A relationship correction can change an
endpoint or effective interval; filtering older matching revisions without
first selecting the latest known revision would create false graph edges.

## Decision

`GET /api/v1/tenants/{tenant_id}/assets/{asset_id}/relationships` requires
signed Unix-millisecond `effective_at_ms` and `known_at_ms` query parameters.
`limit` defaults to 50 and must be 1 through 100. `after` is an optional,
exclusive relationship-ID cursor. Results are ordered by stable relationship
ID and include a `next_after` cursor only if another row exists. The response
contains the exact revision of each matching edge, using the same projection
as the exact-revision route.

The adapter selects candidate identities known at the recorded cutoff, then
their latest known revisions. It applies current endpoints and half-open
effective intervals only after that selection. PostgreSQL fetches at most
`limit + 1` result rows. A separate check against the current tenant asset
catalogue distinguishes an empty graph (`200`) from a missing asset (`404`);
asset existence itself is not historical.

The route reuses bearer authentication and a fresh exact-tenant
`metadata.read` grant. Invalid input returns `400`, and responses use the
existing generic authorization and operational errors and `no-store` policy.
It is mounted only with complete bearer identity configuration. The existing
unbounded repository read remains an internal capability; HTTP uses only the
bounded page operation.

## Consequences

Clients can traverse one asset's outgoing and incoming relationships without
assuming a vehicle-specific hierarchy. Both time cutoffs and the cursor must
be repeated unchanged across pages. The recorded cutoff is a data filter, not
an MVCC snapshot: a newly committed backdated fact can change later pages.
Clients needing a stable traversal should restart from the first page after
such a change. There is no cross-request snapshot lease.

The result cap bounds response size and row reconstruction, but finding the
latest known revision can still examine many candidate identities for a
high-degree asset. Query plans and latency need measurement before claiming a
large-graph capacity target.

## Alternatives considered

An unbounded HTTP list was rejected because tenant graphs can grow without a
small natural maximum. Offset pagination was rejected because inserts before
the offset can shift later results. A current-time default was rejected
because it would make graph meaning depend on request timing.
