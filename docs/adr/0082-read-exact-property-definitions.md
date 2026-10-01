# ADR 0082: Read exact property definitions through protected HTTP

## Status

Accepted, 1 October 2026.

Supersedes: none. Superseded by: none.

## Context

[ADR 0081](0081-read-exact-asset-type-versions.md) exposes an asset type's
ordered property ID/version references, but an HTTP client cannot resolve a
reference to its name, value category, or canonical unit. Those fields are
needed to interpret an asset's semantic state without relying on device-specific
parameter names. The metadata application service and PostgreSQL adapter
already retrieve one exact tenant-scoped property-definition version.

## Decision

`GET /api/v1/tenants/{tenant_id}/properties/{property_id}/versions/{version}`
returns one exact property definition with `id`, `version`, `name`, `value_kind`,
and `canonical_unit`. The value kind is one of `boolean`, `integer`, `decimal`,
or `text`. `canonical_unit` is a unit identifier for a numeric property or
`null` when the definition has no unit. It is never inferred from the value
kind. Unit identifiers retain their configured text; this endpoint does not
convert measurements or assert a particular unit catalogue.

The route shares the configured bearer credential, registered actor mapping,
and fresh exact tenant `metadata.read` grant used by the protected asset-type
query. Only consumed authorization evidence scopes repository lookup. The
version path component must be a positive decimal integer fitting `u32`.
Invalid identifiers or versions return `400`; an absent exact version returns
`404` without a latest-version fallback. Authentication, denial, operational
failures, method handling, and `Cache-Control: no-store` follow the existing
protected metadata conventions. The route is mounted only with complete bearer
identity configuration.

No domain, schema, migration, or SQLx query change is required.

## Consequences

Clients can follow an asset type's pinned property references to stable
semantic meaning. The response is a transport DTO, not a persistence row or
serialized domain aggregate. A property can be shared by several asset types;
the URL therefore belongs to the tenant's property catalogue rather than one
asset or type. Current authorization is evaluated on every request, including
after grant revocation.

This endpoint does not list properties, resolve an implicit latest version,
publish definitions, provide measured values, or convert units. The configured
single-principal bearer mechanism remains limited as recorded in
[ADR 0079](0079-protect-initial-asset-query.md).

## Alternatives considered

Inlining property definitions in every asset-type response was rejected because
it would enlarge that contract and couple separate immutable definition
lifecycles. Returning a latest version was rejected because the asset type
references one exact version. Reusing a nested asset-type URL was rejected
because property definitions can be shared across asset types.
