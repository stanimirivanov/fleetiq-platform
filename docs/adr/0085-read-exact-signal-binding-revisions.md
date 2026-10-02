# ADR 0085: Read exact signal-binding revisions through protected HTTP

## Status

Accepted, 2 October 2026.

Supersedes: none. Superseded by: none.

## Context

Signal bindings explain how a decoded device signal is attributed to an exact
property definition on a tenant-owned asset. The metadata application service
and PostgreSQL adapter can retrieve an immutable recorded revision, but HTTP
clients cannot inspect that attribution. A diagnostic read must distinguish
the source device topology from the semantic target and must not imply that a
recorded revision is the binding effective at the present time.

## Decision

`GET /api/v1/tenants/{tenant_id}/signal-bindings/{binding_id}/revisions/{revision}`
returns one exact recorded binding revision. The response includes its stable
ID, tenant, decimal-string revision, recorded Unix milliseconds, source device
asset, endpoint and signal IDs, target asset and exact property-definition
reference, and half-open physical effective interval. The string preserves the
full positive `u64` revision range for JavaScript consumers. The path accepts
only a positive decimal integer fitting that range.

The route reuses configured bearer authentication, current actor resolution,
and an exact tenant `metadata.read` grant on every request. The existing
application service derives repository tenant scope only from consumed grant
evidence. Invalid identifiers or revisions return `400`; an absent exact
revision returns `404` without selecting another revision. Authentication,
denial, operational failures, method handling, and `Cache-Control: no-store`
follow the protected metadata-read conventions. The route is absent without
complete bearer identity configuration.

No schema, checked SQL query, or domain model change is required. The
relationship and binding routes share one parser for their identical recorded
revision path contract.

## Consequences

An authorized client can inspect which source signal was mapped to which
semantic asset property at a specific recorded revision. It can resolve the
target property through the separate exact-definition endpoint. The result is
a transport DTO rather than a serialized domain aggregate or storage row.

The endpoint does not execute semantic resolution or select bindings at a
known- and effective-time pair. Those require separate bounded query designs.
Revoking the actor's grant affects the next request. The single-principal
bearer limitations recorded in [ADR 0079](0079-protect-initial-asset-query.md)
remain.

## Alternatives considered

An implicit latest revision was rejected because corrections can change the
source, target, or effective interval. An unbounded source-snapshot endpoint
was deferred because it needs explicit collection and time-selection rules. A
JSON number for revision was rejected because IEEE 754 clients cannot exactly
represent every valid `u64` value.
