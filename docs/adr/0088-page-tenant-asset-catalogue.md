# ADR 0088: Page the tenant asset catalogue through protected HTTP

## Status

Accepted, 2 October 2026.

Supersedes: none. Superseded by: none.

## Context

Clients can read an asset by ID and inspect its relationships, but cannot
discover asset IDs without a separate catalogue. A tenant may contain many
assets, and each asset can have an unbounded number of external identifiers.
An unbounded list or complete aggregates would make the first exploration
request costly and leave its response size uncontrolled.

## Decision

`GET /api/v1/tenants/{tenant_id}/assets` returns asset summaries containing
stable ID, tenant ID, display name, and pinned asset-type version in ascending
stable asset-ID order. `limit` defaults to 50 and must be 1 through 100.
Optional `after` is an exclusive asset-ID cursor. `next_after` is present only
when another asset exists. An authorized empty catalogue returns `200`.
External identifiers remain available through the exact-asset route.

The adapter fetches at most `limit + 1` tenant-scoped asset rows in one indexed
query. The tenant scope comes only from consumed `ReadMetadata` authorization
evidence. The route reuses
configured bearer authentication, a fresh exact-tenant `metadata.read` grant,
generic protected errors, and `Cache-Control: no-store`. Invalid query input
returns `400`; the route is absent without complete bearer configuration.

## Consequences

An operator client can discover assets and then follow exact asset and
relationship links without assuming a particular vehicle or equipment model.
Cursor traversal does not freeze writes across requests. A newly inserted
asset with an ID before or at the cursor will not appear later in that
traversal; restart when a complete current catalogue is required. There is no
name search, filtering, count, or cross-request snapshot token.

The existing tenant-leading asset primary key supports this query. No
migration or new dependency is required.

## Alternatives considered

An unbounded list was rejected because tenants can grow. Offset pagination
was rejected because concurrent insertions can shift results. Fetching each
asset through the exact lookup was rejected because it would issue a query
per result. Including external identifiers was rejected because their
per-asset count is not bounded. Name-based ordering was rejected because
names are not unique or stable identities.
