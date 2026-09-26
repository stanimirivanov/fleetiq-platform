# ADR 0049: Separate platform authorization from tenant authorization

## Status

Accepted, 26 September 2026.

Supersedes: none. Superseded by: none.

## Context

FleetIQ protects tenant-owned operations with exact actor, tenant, and action
grants. The next product-foundation capability must provision tenants, but a
tenant-scoped authorization decision cannot protect creation of a tenant that
does not exist yet. Reusing a tenant's `ManageAccess` grant would also let a
tenant administrator perform platform-wide operations outside that tenant's
boundary.

Authentication proves an internal actor identity but grants neither tenant nor
platform authority. Tenant provisioning therefore needs a separate policy
scope before its command and audit behavior are implemented.

## Decision

FleetIQ defines a closed `PlatformAction` vocabulary distinct from
`TenantAction`. Its first and only action is `ProvisionTenant`. A platform
authorization request contains the authenticated internal actor and exact
platform action; it deliberately contains no tenant identifier.

`PlatformAuthorizationService` consumes `AuthenticatedActor` evidence and
evaluates the complete request through the consumer-owned
`PlatformAccessPolicy` port. Only an explicit allow result creates
`AuthorizedPlatformAction`. Policy absence and policy failures fail closed.
The evidence has no public constructor and is consumed by a future protected
platform operation.

PostgreSQL stores current grants in
`identity.platform_action_grants` under the exact `(actor_id, action)` primary
key. The actor must already exist, and a database check constraint mirrors the
closed application vocabulary. The policy adapter performs one current-state
existence query per authorization attempt so revocation affects the next
decision without cache delay.

Platform grants imply no tenant grants, tenant membership, or access to tenant
data. Tenant grants imply no platform authority. Adding a platform action
requires an exhaustive Rust change and a forward database migration.

## Consequences

Tenant provisioning can be implemented next without inventing a nonexistent
tenant scope or giving tenant administrators platform authority. Other future
platform operations must justify a new explicit action rather than inheriting
`ProvisionTenant`.

The system now has two authorization scopes with intentionally different
request shapes. Adapters must select the scope from the protected operation,
not from caller-controlled claims or route parameters.

Platform grant mutation, immutable grant-change audit, initial operator
bootstrap, actor provisioning, and tenant provisioning remain separate
capabilities. Until controlled administration exists, development environments
may seed actors and grants directly. Product routes must not expose direct
mutation of the grant table.

## Alternatives considered

Authorizing tenant creation with `ManageAccess` on the new tenant was rejected
because policy cannot reference a tenant before that tenant exists.

Authorizing tenant creation with `ManageAccess` on any existing tenant was
rejected because authority inside one tenant must not confer platform-wide
control.

Treating every authenticated actor as a platform administrator was rejected
because authentication establishes identity rather than authority.

Using one action enum with an optional tenant identifier was rejected because
the optional field would permit invalid combinations and make policy adapters
interpret scope dynamically.

Adding roles or action hierarchy was rejected because only one platform action
is currently required and inheritance would create authority that is harder to
audit and revoke precisely.
