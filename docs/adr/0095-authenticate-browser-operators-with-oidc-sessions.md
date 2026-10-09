# ADR 0095: Authenticate browser operators with OIDC server sessions

## Status

Accepted, 9 October 2026.

Supersedes: none. Superseded by: none.

## Context

The semantic metadata contract is published, but the configured workload bearer
identifies only one principal and must never enter browser code. The user chose
provider-neutral OpenID Connect (OIDC), server-managed sessions, and one public
origin for the UI and API. The provider is configured at deployment. Existing
actor mapping and tenant policy must remain authoritative.

## Decision

The server owns authorization-code login with S256 PKCE, browser-bound one-use
state, and nonce verification. The maintained OIDC library checks signed ID
tokens, exact issuer, audience, expiry, nonce, and access-token hash when
present; the adapter also checks authorized-party claims. Discovery uses a
fixed configured HTTPS issuer, bounded responses, timeouts, and no redirects.
Provider metadata and public keys are refreshed on each callback.

PostgreSQL stores five-minute login attempts and opaque authenticated sessions.
A callback atomically consumes its matching state and browser-cookie digest
once. Successful login resolves an already provisioned active operator,
removes the prior session, and creates a fresh random credential. Both login
attempts and sessions retain a digest of issuer, client, and authority; changing
that trust configuration invalidates previous credentials without invalidating
sessions merely for client-secret rotation. The cookie
is Secure, HttpOnly, SameSite=Lax, host-only, Path=/, with the __Host- prefix.
The session table retains the credential digest, provider trust binding,
external identity, original actor, and absolute expiry. Provider tokens are discarded after
verification. Expiry is the earlier of eight hours and verified ID-token expiry;
there is no sliding expiry or refresh-token storage.

Every protected request validates the session, resolves the current active
external identity, and compares the result with the original actor. Unlinking,
suspension, or reassignment cannot transfer an existing session. Every metadata
read then evaluates current tenant metadata.read policy. Sessions cache no
grants. Missing, invalid, expired, or unregistered credentials return bounded
401 problem details, without provider redirects; denied tenant access is 403.

UI and API share one configured HTTPS public origin. Logout requires the exact
Origin and X-FleetIQ-CSRF: 1, deletes the session and bound pending attempts,
and clears cookies. A callback already consumed before sign-out may finish;
there is no cancellation-generation mechanism for concurrent sign-in. No
credentialed CORS is enabled. Redirects use fixed configuration; request Host,
forwarding headers, and return-url parameters cannot choose destinations.
Browser and configured workload modes are mutually exclusive on one listener.
ADR 0079 continues to govern trusted workload deployments.

The server owns HTTP/OIDC policy; the PostgreSQL adapter owns checked SQL.
Domain and capability crates gain no HTTP, cookie, or provider dependencies.
The [operations guide](../operations/browser-identity.md) defines configuration
and rollout. The [modular contract](../../contracts/http/openapi/openapi.yaml)
adds browser operations without changing metadata payloads.

## Consequences

Connected UX can use same-origin requests without workload secrets. Actor and
grant provisioning remain explicit trusted administration; email, provider
roles, and groups cannot create FleetIQ access. Session JSON exposes only the
internal actor ID and expiry. Multiple backend instances share PostgreSQL and
atomic callback consumption. Stored attempts and sessions are immutable except
for deletion. Cleanup removes expired records without deciding validity.

Actor, mapping, tenant, and grant revocation apply to subsequent reads.
Provider-only revocation is bounded by session expiry; back-channel logout,
introspection, and administrative session termination remain later work. This
slice establishes an authentication boundary, not a qualified production
provider or deployment. Connected UX and its operator guide remain next.

The schema is additive and may remain during rollback. Disable browser
configuration and restart to remove browser routes and cookie authentication.
Trusted workload mode can be restored separately; it is never a UI fallback.

The library introduces the rsa dependency advisory assessed in
[the dependency review](../development/plans/browser-identity-dependency-review.md).
The project owner explicitly approved its narrow verification-only exception
on 9 October 2026. The review records the exact dependency path, exposure, owner,
and removal conditions; it does not declare rsa patched or disable other
advisories. Private-key use or changes to this path require a fresh review.

## Alternatives considered

Browser-held bearer tokens add token handling to the SolidJS runtime and were
not selected. A workload-secret proxy cannot identify the operator and was
rejected. Provider-specific SDKs would couple the identity model to a customer
provider. Automatic provisioning from email or roles bypasses administration.

Generic mutable session middleware was evaluated. Typed immutable sessions and
atomic one-use login consumption match this boundary and avoid concurrent
session updates or an additional SQLx version through a generic store. OIDC
verification, cookie parsing, OS randomness, URL validation, and hashing use
maintained libraries.
