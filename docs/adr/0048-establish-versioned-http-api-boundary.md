# ADR 0048: Establish a versioned HTTP API boundary

## Status

Accepted, 26 September 2026.

Supersedes: none. Superseded by: none.

## Context

The backend composition root exposes operational health probes but no product
HTTP contract. Tenant-owned metadata and access-administration use cases are
available behind application boundaries, and authentication now produces
typed actor evidence. Adding business routes one at a time without first
selecting a versioning and error-response convention would make each handler
choose compatibility behavior independently.

The concrete identity provider remains undecided. The initial HTTP slice must
therefore prove routing and response compatibility without exposing protected
data or introducing a temporary authentication mechanism.

## Decision

Product HTTP contracts use a major version in the path, beginning with
`/api/v1`. `GET /api/v1` is an unauthenticated discovery resource that returns
the service name and active major version as `application/json`. It contains no
tenant, identity, dependency, build, or configuration data.

HTTP API failures use bounded problem-details documents with media type
`application/problem+json`. The initial representation contains only:

- `type`, a stable FleetIQ URN identifying the problem category;
- `title`, a short category-level description; and
- `status`, the HTTP status code repeated in the body.

Unknown resources return `404 Not Found`. Unsupported methods on a known
resource return `405 Method Not Allowed` and an accurate `Allow` header. Error
documents do not echo paths, query values, request bodies, credentials, tenant
identifiers, or internal error messages. Later errors may add optional fields
when they have defined disclosure and compatibility semantics.

Operational probes remain outside `/api/v1` because orchestrators consume them
rather than product API clients. Their established empty response behavior is
unchanged.

Compatible additions may extend version 1 with new resources, optional response
members, and new problem types. Removing or changing existing meanings requires
a new major path or an explicit compatibility and migration plan.

## Consequences

Clients can identify the supported major contract before protected routes are
available. Future handlers have one media type and stable minimum shape for
safe errors, and version changes are visible at the routing boundary.

The discovery endpoint deliberately says nothing about enabled product
capabilities. Authentication challenges, authorization failures, validation
errors, pagination, request correlation, and business resource schemas remain
separate slices that must reuse or deliberately extend this boundary.

Axum's JSON feature and Serde derive support become direct transport-layer
dependencies. No domain or application crate depends on HTTP or serialization.

## Alternatives considered

An unversioned `/api` path was rejected because later breaking changes would
need content negotiation or coordinated replacement of every client.

Media-type versioning was rejected for the initial system because path versions
are easier to route, observe, document, and operate while there is one backend
and one primary client.

Returning plain text or empty bodies for API failures was rejected because
clients need a machine-readable, extensible error category independent of
localized prose.

Publishing protected metadata routes with a development-only identity header
was rejected because it would create a caller-controlled authentication bypass
in the production composition root.
