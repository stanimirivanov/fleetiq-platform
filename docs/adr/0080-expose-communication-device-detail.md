# ADR 0080: Expose communication capability beneath its asset

## Status

Accepted, 1 October 2026.

Supersedes: none. Superseded by: none.

## Context

The first protected HTTP route returns an asset's identity, type, name, and
external identifiers. A communication device is already modeled and persisted
as a capability of an existing asset, with ordered endpoints. Operators need
to inspect those endpoint identities before signal bindings and semantic graph
views can be useful. A second route must not create a parallel device identity
or a different authentication and tenant-scope rule.

## Decision

`GET /api/v1/tenants/{tenant_id}/assets/{asset_id}/device` returns the
communication capability attached to that exact asset. Its JSON document
contains the owning tenant and asset IDs and an `endpoints` array in declared
order. Each endpoint has `id`, `kind`, and `name`. The route uses the configured
bearer identity, exact actor resolution, and a fresh `metadata.read` grant
evaluation for the selected tenant on every request. It passes only the
consumed authorization evidence and asset ID to the device application service.

A valid request for an asset without communication capability returns `404`,
as does a missing asset. Invalid path identifiers return `400`; rejected or
unregistered credentials return the existing generic `401` challenge; denied
policy returns `403`. Operational failures return bounded `500` problem
details. The protected response uses `Cache-Control: no-store` and the
version-one API's existing method and problem conventions.

Authentication, metadata-read authorization, and safe HTTP error mapping are
shared by the asset and device handlers in a server-local module. Domain and
application crates remain independent of HTTP. The route is mounted only
when the already accepted bearer configuration is complete.

## Consequences

The first two metadata reads follow one credential-to-actor-to-current-grant
sequence while retaining separate asset and device application ports. No new
schema, credential mechanism, policy action, or tenant inference is needed.
The database foreign key ensures a persisted device belongs to an existing
asset, so the handler does not issue an additional parent-asset lookup.

The configured bearer remains a single-principal development adapter under
[ADR 0079](0079-protect-initial-asset-query.md); this route does not make it a
production identity provider. Endpoint declarations may grow with a device;
response-size and pagination policy will need review before large endpoint
collections are supported through a public deployment.

## Alternatives considered

A top-level `/devices/{device_id}` resource was rejected because it suggests a
second identity despite the domain's device-as-asset model.

Returning endpoint declarations within every asset response was rejected
because most assets are not communication devices and the existing asset
document has a distinct contract.

Reimplementing bearer parsing and policy mapping in the new handler was
rejected because identical security behavior would drift between routes.
