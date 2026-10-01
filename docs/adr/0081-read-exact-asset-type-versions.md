# ADR 0081: Read exact asset-type versions through protected HTTP

## Status

Accepted, 1 October 2026.

Supersedes: none. Superseded by: none.

## Context

The protected asset query returns an asset's pinned asset-type ID and version,
but HTTP clients cannot inspect that immutable definition. The operator view
needs the type's name and the property definitions it supports without
silently substituting a newer version or exposing another tenant's meaning.
The metadata application service and PostgreSQL adapter already support exact
asset-type-version lookup.

## Decision

`GET /api/v1/tenants/{tenant_id}/asset-types/{type_id}/versions/{version}`
returns one exact asset-type definition. Its JSON representation contains the
type ID, positive version, name, and supported property ID/version references
in declaration order. It does not inline property-definition bodies.

The route uses the configured bearer credential, registered actor mapping,
and a fresh exact `metadata.read` grant for the selected tenant on each
request. Only consumed authorization evidence scopes the metadata repository.
The version path component must be a positive decimal integer fitting `u32`;
invalid identifiers or versions return `400`. An absent exact version returns
`404` and never falls back to another version. Authentication, denial,
operational failures, method handling, and `Cache-Control: no-store` follow
the established protected metadata conventions.

The route is mounted only when the existing HTTP bearer identity configuration
is complete. No schema or domain type changes are required.

## Consequences

Clients can follow an asset's pinned type reference to reproducible meaning.
Tenant policy is checked before definition lookup, and the response is a
transport DTO rather than a serialized domain aggregate. Property references
remain explicit so their own definitions can be queried through a later
bounded endpoint without coupling this contract to nested expansion.

This does not provide type listing, latest-version selection, property-detail
lookup, or definition publication over HTTP. Large supported-property arrays
will require a response-size policy before public deployment. The configured
single-principal bearer mechanism remains limited as recorded in
[ADR 0079](0079-protect-initial-asset-query.md).

## Alternatives considered

Returning the latest version for an unspecified or missing version was
rejected because an asset pins one immutable version and implicit upgrades
would change its meaning.

Embedding the entire asset-type and property graph in the asset response was
rejected because it would make that existing document large and tie separate
definition lifecycles to one response contract.

Exposing SQL rows directly was rejected because persistence layout and public
representation have different compatibility obligations.
