# ADR 0087: Page bitemporal source bindings through protected HTTP

## Status

Accepted, 2 October 2026.

Supersedes: none. Superseded by: none.

## Context

The metadata and semantic capabilities already resolve signal bindings for a
decoded source at explicit physical and recorded times. Their unbounded read
is for internal resolution and cannot safely serve a growing HTTP collection.
An operator also needs to see the recorded binding revisions that explain
which asset properties a device signal could populate.

## Decision

`GET /api/v1/tenants/{tenant_id}/signal-bindings` requires the source tuple
`device_asset_id`, `endpoint_id`, and `signal_id`, plus signed Unix-millisecond
`effective_at_ms` and `known_at_ms` query parameters. `limit` defaults to 50
and must be 1 through 100. `after` is an optional exclusive binding-ID
cursor. Results are ordered by stable binding ID; `next_after` is present only
when another row exists. Each item uses the exact-revision HTTP projection.

The adapter discovers identities that selected the source by the recorded
cutoff, selects each latest known revision, then applies current source and
half-open effective-time filters. PostgreSQL returns at most `limit + 1`
rows. The existing unbounded internal read remains available to semantic
resolution; HTTP uses only the bounded page operation.

A source signal ID is emitted by a decoder or profile and has no independent
catalogue record. A valid source tuple with no matching bindings returns an
empty `200` page, even if it has never been observed. The route reuses bearer
authentication and a fresh exact-tenant `metadata.read` grant. Invalid query
input returns `400`; other security, error, and `no-store` behavior follows
protected metadata reads. It is mounted only with complete bearer identity
configuration.

## Consequences

Clients can inspect attribution without assuming an industry-specific asset
model or interpreting decoded telemetry as a semantic property. They must
repeat both time cutoffs and the same source tuple across pages. The recorded
cutoff is a data filter, not a cross-request database snapshot: a backdated
commit can change later pages, requiring a traversal restart for consistency.

The page limit bounds response size and reconstruction, but finding latest
revisions can still examine many candidate identities for a high-degree
source. Large-source capacity needs measured query plans and latency.

## Alternatives considered

An unbounded HTTP list was rejected because the source may accrue many
bindings. Offset pagination was rejected because inserts can shift results.
Implicit current time was rejected because it changes historical meaning
between requests. Returning `404` for an unmapped signal was rejected because
the signal has no registration lifecycle to establish its existence.
