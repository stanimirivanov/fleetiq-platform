# ADR 0050: Provision tenants atomically with initial access

## Status

Accepted, 26 September 2026.

Supersedes: none. Superseded by: none.

## Context

FleetIQ now distinguishes the platform-wide `ProvisionTenant` action from
tenant-scoped authorization. Creating only a tenant row would leave an
unmanageable tenant, while independently creating access grants or audit
evidence could leave partial state after failure. Provisioning also needs safe
retry behavior because a client can lose the successful response after the
transaction commits.

Both the host-assigned provisioning identity and requested tenant identity can
arbitrate concurrent commands. The repository must resolve conflicts on either
identity without allowing one identifier to name different immutable content.

## Decision

`TenantProvisioningService` consumes exact `AuthorizedPlatformAction` evidence
for `ProvisionTenant`. Callers provide a host-assigned `TenantProvisioningId`,
the new `TenantId`, an existing initial-administrator `ActorId`, and trusted
host time. The service derives the authenticated provisioner actor from the
authorization evidence.

One PostgreSQL transaction creates:

1. an immutable row in `identity.tenant_provisionings`;
2. the tenant row in `metadata.tenants`; and
3. exact `metadata.read`, `metadata.manage`, and `access.manage` grants for the
   initial administrator.

The initial administrator must already be a valid internal actor. Provisioning
does not create an actor or external-identity mapping.

The provisioning row has a primary key on `provisioning_id` and a unique key on
`tenant_id`. The repository first attempts the audit insert with
`ON CONFLICT DO NOTHING`. Its tenant foreign key is deferred until commit so
the tenant may be inserted later in the same transaction. PostgreSQL therefore
serializes concurrent contenders on either unique arbiter before any tenant or
grant state is committed.

When the audit insert encounters a conflict, the repository reads all rows
matching either identity in a new statement. Exactly one row with every
immutable field equal is an exact retry and returns `AlreadyProvisioned`.
Different content, including reuse of either unique identity, is an identity
collision. The fresh statement after conflict observes the concurrent winner
under PostgreSQL's default `READ COMMITTED` isolation.

The three grants use one set-based insert. Any failed actor reference,
constraint, tenant insert, grant insert, or deferred foreign-key check rolls
back the provisioning record and every dependent write. Provisioning audit
rows reject update and delete operations through a database trigger.

## Consequences

A successful outcome always leaves a usable tenant with one initial
administrator and immutable provenance. An exact retry is safe after an
ambiguous response, including when two identical commands execute
concurrently.

The initial administrator receives all current tenant actions explicitly. A
future tenant action is not granted automatically: changing initial access
requires an explicit migration and provisioning-policy decision.

Tenant identity becomes permanent while its immutable provisioning record
references it. Tenant suspension, closure, deletion, ownership transfer, and
administrator replacement require separate lifecycle commands and audit
contracts.

The transaction does not provision identities or platform grants. Initial
operator bootstrap and creation of the initial administrator remain controlled
prerequisites outside this command.

## Alternatives considered

Creating the tenant and grants in separate commands was rejected because
failures could leave an inaccessible tenant or untracked authorization state.

Using only the provisioning identity as an idempotency key was rejected because
two commands with different IDs could race to claim the same tenant.

Checking for existing rows before inserting was rejected because concurrent
commands could both observe absence. Unique constraints are the durable
arbiters.

Granting only `access.manage` was rejected because the initial administrator
could delegate access but could not inspect or configure the tenant metadata.

Automatically creating the initial actor from caller-supplied identity data was
rejected because actor and external-identity provisioning require their own
proof, collision, and audit rules.
