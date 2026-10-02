# ADR 0084: Read exact relationship revisions through protected HTTP

## Status

Accepted, 1 October 2026.

Supersedes: none. Superseded by: none.

## Context

The metadata application service and PostgreSQL adapter can read an immutable
recorded revision of a tenant-owned directed asset relationship. The protected
HTTP surface exposes assets and definition meanings, but cannot yet return a
recorded edge. A client needs the exact revision, its pinned type, endpoints,
recorded time, and physical effective interval without conflating a recorded
fact with a current graph snapshot.

## Decision

`GET /api/v1/tenants/{tenant_id}/relationships/{relationship_id}/revisions/{revision}`
returns one exact recorded relationship revision. The response includes its
tenant, stable ID, decimal-string revision, recorded Unix milliseconds, pinned
relationship-type ID and version, directed source and target asset IDs, and a
half-open effective interval in Unix milliseconds. The string preserves the
full positive `u64` revision range for JavaScript consumers; the path accepts
only a positive decimal integer fitting that range.

The route reuses configured bearer authentication, current actor resolution,
and an exact tenant `metadata.read` grant on every request. The application
service derives repository tenant scope solely from consumed authorization
evidence. Invalid identifiers and revisions return `400`; an absent exact
revision returns `404` without selecting another revision. Existing protected
route rules govern generic authentication, denial, operational errors, method
handling, and `Cache-Control: no-store`. The route is mounted only with complete
bearer identity configuration.

No schema, checked SQL query, or domain model change is required.

## Consequences

Clients can inspect a recorded edge and follow its pinned relationship-type
version to the separate definition endpoint. A later graph snapshot query can
select revisions using independent effective- and known-time cutoffs. This
endpoint does not imply the returned revision is current or effective now.

The response is a transport DTO; database rows and domain aggregates remain
behind their adapters. Current policy is evaluated before each lookup, so
grant revocation takes effect on the next request. The single-principal bearer
limitations from [ADR 0079](0079-protect-initial-asset-query.md) remain.

## Alternatives considered

An implicit latest revision was rejected because a correction may change
endpoints or effective dates and would obscure which recorded fact the client
requested. A graph snapshot query was deferred because it introduces separate
time-selection and collection-bounding decisions. A JSON number for revision
was rejected because clients using IEEE 754 numbers cannot exactly represent
every valid `u64` revision.
