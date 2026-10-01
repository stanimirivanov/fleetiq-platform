# ADR 0079: Protect the first asset query with a configured bearer identity

## Status

Accepted, 1 October 2026.

Supersedes: none. Superseded by: none.

## Context

FleetIQ has a provider-neutral authentication and authorization boundary but
no selected production identity provider. A protected HTTP query must validate
an actual credential before actor resolution, evaluate current tenant policy,
and keep transport values outside application and domain crates. A caller-
supplied actor or identity header would bypass that sequence. Delaying every
product route until workforce identity selection would prevent an end-to-end
reference implementation.

## Decision

The first product query is `GET /api/v1/tenants/{tenant_id}/assets/{asset_id}`.
The backend enables it only when a 64-character hexadecimal bearer credential,
a trusted authority ID, and an opaque subject are configured together. The
credential is a randomly generated 256-bit secret. The server compares its
fixed-width bytes in constant time and derives the external identity solely
from process configuration. It resolves that identity to a registered active
actor, evaluates the current exact `metadata.read` grant for the selected active
tenant, and passes the resulting typed evidence to the existing asset service.
No grant or result is cached.

Missing, malformed, invalid, or unregistered credentials return one generic
`401` challenge. A missing current grant returns `403`. Invalid path identifiers
return `400` after authentication; an absent asset returns `404`. Operational
failures return bounded `500` problem details without internal causes. Protected
responses carry `Cache-Control: no-store`. The route is absent when credential
configuration is omitted; partial configuration fails process startup.

The configured credential represents exactly one external identity. Its
mapping, actor, tenant, grant, and asset must be provisioned through the
existing trusted administration and metadata boundaries. Non-loopback use
requires TLS termination and secret injection by the deployment environment.
The credential must be rotated by replacing it and restarting the process.

## Consequences

The route exercises credential validation, exact actor mapping, current policy,
tenant-derived lookup, and HTTP projection through the real PostgreSQL adapters.
It creates no SQL schema or domain dependency on HTTP. The bearer secret and
external subject are omitted from debug output and routine errors.

A single long-lived configured credential is intentionally limited. It is not
a workforce login, a multi-principal token store, or a production identity-
provider integration. Production identity selection, key rotation without
restart, rate limiting, request audit, and public-network deployment controls
remain separate work. Revocation of the actor mapping or grant takes effect on
the next request because the database is queried for each operation.

## Alternatives considered

A caller-selected actor or trusted-looking HTTP identity header was rejected:
neither proves credential possession and both can bypass identity mapping.

Making a provider-specific OIDC selection here was deferred because issuer,
audience, key rotation, and customer identity requirements are not yet known.

Mounting a route with an injected test authenticator but no concrete process
adapter was rejected because it would leave the reference slice unrunnable.
