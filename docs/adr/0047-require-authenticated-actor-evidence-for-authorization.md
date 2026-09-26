# ADR 0047: Require authenticated actor evidence for tenant authorization

## Status

Accepted, 26 September 2026.

Supersedes: none. Superseded by: none.

## Context

FleetIQ authenticates provider-specific credentials, resolves the resulting
external identity to a stable internal actor, and then evaluates an exact actor,
tenant, and action tuple. The first authorization API accepted a publicly
constructible `TenantAccessRequest` containing a bare `ActorId`. That contract
documented that the actor must come from authentication, but the type system did
not enforce the requirement. A transport or future application adapter could
therefore authorize a caller-selected actor identifier without first validating
credentials and resolving the principal.

The caller authentication service now produces `AuthenticatedActor` evidence
with no public constructor. Tenant authorization should consume that evidence
directly so the public API preserves the intended trust chain.

## Decision

`TenantAuthorizationService::authorize` consumes one `AuthenticatedActor`, one
selected `TenantId`, and one `TenantAction`. It constructs the complete
`TenantAccessRequest` privately and evaluates the configured policy. Public code
cannot construct a policy request from a bare actor identifier.

Authentication evidence is consumed by one authorization attempt. Successful
authorization produces the existing `AuthorizedTenantAction`, which is then
consumed by one protected use case. Authentication does not itself grant tenant
access, and tenant selection remains untrusted routing input until policy allows
the exact tuple.

The `TenantAccessPolicy` port continues to receive a read-only
`TenantAccessRequest` so persistence and future policy adapters can evaluate its
actor, tenant, and action. Construction remains private to the application
authorization service.

Application-internal tests may construct authenticated evidence through a
crate-private helper. Integration tests cross the same public authentication
service used by adapters, using deterministic in-memory test ports rather than
a public bypass constructor.

## Consequences

The public Rust API now enforces the sequence credentials → external identity →
internal actor → tenant authorization → protected operation. Possession of an
`ActorId`, including one parsed from a request, cannot directly enter tenant
policy evaluation.

Callers must retain and pass typed authenticated evidence rather than copying an
actor identifier into an authorization request. HTTP middleware and other
transports will need to place that evidence into request processing and consume
it when authorizing an operation.

This is a source-breaking application API change made before a product API is
released. It changes no persisted representation, migration, action vocabulary,
or grant semantics. Rollback would restore the weaker public request constructor
and is therefore discouraged.

## Alternatives considered

Keeping the public request constructor and relying on documentation was rejected
because a missed authentication call would compile and could become an actor
impersonation vulnerability.

Accepting both `AuthenticatedActor` and bare `ActorId` overloads was rejected
because the convenience overload would preserve the bypass path.

Combining authentication and tenant authorization into one provider-specific
service was rejected because credential validation and tenant policy have
different responsibilities, failure modes, and replacement boundaries.

Keeping reusable authenticated evidence but consuming it by reference was
rejected for the initial contract because consuming the value makes each
authorization attempt explicit and discourages accidental reuse across
independent requests.
