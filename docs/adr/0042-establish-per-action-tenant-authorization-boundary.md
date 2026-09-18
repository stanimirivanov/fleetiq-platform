# ADR 0042: Establish a per-action tenant authorization boundary

## Status

Accepted, 18 September 2026.

Supersedes: none. Superseded by: none.

## Context

FleetIQ stores tenant-owned metadata and is preparing to expose product and
administrative APIs. Existing repository methods require a `TenantId`, but a
tenant identifier supplied through an HTTP route, header, token claim, or
request body is routing input rather than proof that the caller may access the
tenant. Passing authenticated identity and tenant identifiers directly into
protected use cases would make it easy for one missed check to become a
cross-tenant authorization defect.

The production identity provider and detailed access model have not been
selected. FleetIQ needs an application boundary that supports human and
workload identities without coupling domain or application policy to OAuth,
OpenID Connect, provider roles, token claims, or one authorization product.

## Decision

FleetIQ represents a successfully mapped caller with a stable internal
`ActorId`. Authentication adapters will map provider identities to this value;
an actor identifier alone grants no tenant access. Identity-provider subject,
issuer, token, session, and credential details remain adapter concerns.

Every protected tenant operation is described by an explicit
`TenantAccessRequest` containing the authenticated actor, selected tenant, and
application-level `TenantAction`. The initial action vocabulary separates
reading metadata, managing metadata, and managing tenant access. It evolves
with application use cases rather than mirroring provider-specific scopes or
roles. The action enum is closed so an added operation makes exhaustive policy
adapters revisit their decision instead of silently inheriting a grant.

The application-owned `TenantAccessPolicy` port evaluates the complete tuple
against current trusted policy. A missing matching grant is an explicit deny.
Policy lookup or evaluation failure is a separate error and also denies access.
Authentication success, tenant selection, possession of a resource identifier,
or network location never implies authorization.

`TenantAuthorizationService` is the common enforcement point. Only an allow
decision creates `AuthorizedTenantAction`, whose private construction records
the exact actor, tenant, and action that passed policy. Protected application
services should require this evidence instead of a bare tenant identifier. The
evidence is ephemeral: callers must request it for each operation and must not
persist, serialize, cache, or reuse it for another action.

HTTP status mapping, audit records, actor persistence, external-identity
mapping, memberships, roles, and the policy adapter are deferred until their
respective vertical slices. Transport adapters must not reveal whether a
denial resulted from a missing actor, tenant, membership, resource, or grant.

## Consequences

Future protected APIs have one provider-neutral contract for authorizing a
specific operation, and application tests can prove tenant and action isolation
without an identity provider or database. The compiler prevents ordinary code
from manufacturing authorization evidence because its constructor is private.
Policy adapters remain replaceable and may implement role-based, attribute-
based, or relationship-based decisions as product requirements emerge.

The boundary is an architectural guard, not a complete identity system. Code
inside the application crate can still bypass it unless protected use cases are
designed to require `AuthorizedTenantAction`; review and tests must enforce that
rule as routes are added. The initial action vocabulary is intentionally small
and will need more specific variants when a use case requires narrower
privileges.

Policy decisions are evaluated before an operation and can become stale if
retained. Keeping authorization evidence ephemeral limits that interval; later
high-risk or long-running operations may require re-evaluation inside their
transaction or workflow.

## Alternatives considered

Trusting a tenant identifier from the request or a token claim was rejected
because selection and identity assertions do not establish authorization for a
specific operation or resource.

Embedding OAuth scopes, OpenID Connect claims, or provider roles in application
services was rejected because it couples business policy to one transport and
identity provider and makes provider migration part of every use case.

Defining the final role, membership, and policy schema now was rejected because
the administrative workflows and customer access requirements are not yet
known. The port permits those choices to be made in a later end-to-end slice.

Authorizing only tenant membership was rejected because membership alone does
not express the requested operation and would grant broader access than the
application needs.
