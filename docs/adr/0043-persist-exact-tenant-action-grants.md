# ADR 0043: Persist exact tenant action grants in PostgreSQL

## Status

Accepted, 18 September 2026.

Supersedes: none. Superseded by: none.

## Context

[ADR 0042](0042-establish-per-action-tenant-authorization-boundary.md)
established a provider-neutral application contract that authorizes an exact
actor, tenant, and action tuple. FleetIQ now needs a durable policy source for
that contract before protected product APIs can use it. The final identity
provider, administrative workflow, role model, and customer-specific access
requirements remain undecided.

Storing provider roles or token scopes directly would bind application behavior
to an external identity system. Defining role inheritance before the required
jobs and separation of duties are known would also risk grants that are broader
than intended. The persisted model therefore needs to express current policy
without freezing either choice.

## Decision

PostgreSQL stores stable internal actors in `identity.actors`. Actor existence
is not tenant membership and grants no access. External provider identities,
credentials, sessions, and their mapping to an actor remain separate concerns.

PostgreSQL stores each effective authorization fact in
`identity.tenant_action_grants` under the exact composite key `(tenant_id,
actor_id, action)`. Foreign keys require an existing tenant and actor. A closed
check constraint uses the same action vocabulary as the application's
`TenantAction` enum. There are no wildcards, action inheritance, implicit
membership grants, or default tenant.

The PostgreSQL policy adapter evaluates one complete tuple with a single
existence query for every protected operation. It does not cache decisions.
Deleting a grant therefore affects the next evaluation without in-process
invalidation delay. A database failure remains distinct from a missing grant
and fails closed through the application authorization service.

Grant rows represent current authorization state and are mutable through
insertion and deletion. A later administrative application service will own
those changes and append the required audit facts. This migration intentionally
does not claim that direct SQL mutation is a product administration interface.

PostgreSQL row-level security is not enabled. The current backend uses one
trusted application database role and does not set a transaction-local actor
identity that PostgreSQL could use safely in policies. Direct database access
is an operational privilege outside the product authorization boundary. Row-
level security must be reconsidered if untrusted clients receive database
access, database roles become tenant-specific, or a trustworthy request
identity is propagated into database transactions.

## Consequences

The tenant authorization service now has a durable, provider-neutral policy
implementation. Exact keys and foreign keys prevent duplicate, orphaned, and
invalid tenant grant references, while the closed action constraint makes
schema and application vocabulary drift visible during development.

Every protected operation performs a database round trip. This favors immediate
revocation and simple semantics at the current scale. Caching remains possible
later, but it requires explicit maximum staleness, invalidation, outage, and
revocation behavior before adoption.

The actor table initially contains only stable identity. Provisioning,
deactivation, display attributes, external identities, memberships, roles,
delegation, service-account lifecycle, and audit history need later focused
slices. Grant mutations are not yet exposed through application or HTTP APIs.

## Alternatives considered

Persisting roles and role-to-action mappings now was rejected because role
requirements, delegation, and separation of duties are not known. Exact grants
can later be generated from roles without changing the authorization request
contract.

Trusting identity-provider roles or token scopes directly was rejected because
it couples business authorization to provider configuration and makes provider
migration affect protected use cases.

Caching grants in process was rejected because the current workload does not
justify stale-revocation behavior or invalidation complexity.

Enabling PostgreSQL row-level security was deferred because the shared trusted
application role has no safe per-request actor context at the database boundary.
Application queries still retain explicit tenant keys, and product access is
enforced before protected use cases execute.
