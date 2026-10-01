# ADR 0083: Read exact relationship-type versions through protected HTTP

## Status

Accepted, 1 October 2026.

Supersedes: none. Superseded by: none.

## Context

The metadata model stores immutable relationship-type definitions and
revisioned, effective-dated relationships. Its application service and
PostgreSQL adapter already retrieve one exact tenant-scoped definition, but
the protected HTTP catalogue exposes only asset types and properties. A
future graph response needs a stable way to resolve its pinned relationship
type to an operator-facing name without interpreting the identifier itself.

## Decision

`GET /api/v1/tenants/{tenant_id}/relationship-types/{type_id}/versions/{version}`
returns one exact relationship-type definition with `id`, `version`, and
`name`. The definition describes the meaning of a directed edge. It does not
declare allowed endpoint types, cardinality, or graph-shape constraints; those
are not part of the current domain aggregate.

The route uses the configured bearer credential, registered actor mapping,
and a fresh exact tenant `metadata.read` grant on every request. Only consumed
authorization evidence scopes repository lookup. The version path component
must be a positive decimal integer fitting `u32`. Invalid identifiers or
versions return `400`; an absent exact version returns `404` without a
latest-version fallback. Authentication, denial, operational failures,
method handling, and `Cache-Control: no-store` follow the existing protected
definition conventions. The route is mounted only with complete bearer
identity configuration.

No domain, schema, migration, or SQLx query change is required.

## Consequences

Clients can resolve a relationship's pinned type to stable, tenant-owned
meaning once graph reads are available. The response is a transport DTO
rather than a database row or serialized domain aggregate. Current policy is
evaluated before every lookup, including after grant revocation.

The endpoint does not list types, select an implicit latest version, expose
relationship instances, traverse the graph, publish definitions, or add graph
validation rules. The configured single-principal bearer mechanism remains
limited as recorded in [ADR 0079](0079-protect-initial-asset-query.md).

## Alternatives considered

Returning a latest version was rejected because recorded relationships pin
an exact definition version. Nesting the definition beneath an asset was
rejected because a relationship type can be shared by many assets. Adding
endpoint or cardinality fields now was rejected because their semantics and
validation source have not yet been designed.
