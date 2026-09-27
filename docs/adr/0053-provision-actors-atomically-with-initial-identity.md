# ADR 0053: Provision actors atomically with an initial identity

## Status

Accepted, 27 September 2026.

Supersedes: ADR 0051 for the bootstrap operator's initial platform grant set.
Superseded by: ADR 0054 for the bootstrap operator's initial platform grant set.

## Context

FleetIQ can bootstrap its first platform operator and map external identities
to stable internal actors, but it has no authorized operation for registering
later actors. Inserting an actor and its first external identity independently
could leave an unusable actor, an identity mapped to the wrong actor, or no
durable record of who performed the operation.

Actor registration is a platform concern because the actor exists before it
has access to any tenant. It must not reuse `tenant.provision`: creating an
identity and creating a tenant are separate authorities. Actor existence and
identity mapping must also remain separate from authorization grants so that a
new identity receives no access implicitly.

The command may be retried after an uncertain response and may run concurrently
on multiple replicas. Its provisioning ID, actor ID, and exact external
identity can each collide independently, so idempotency must resolve every
unique arbiter.

## Decision

Add the closed `identity.manage` platform action. Platform policy evaluates it
as an exact grant and does not infer it from `tenant.provision` or a tenant
grant. Migration 0016 explicitly gives the action to the existing bootstrap
operator. A new bootstrap grants both current installation authorities:
`identity.manage` and `tenant.provision`. This supersedes ADR 0051 only where it
specified a single initial grant; adding future platform actions still requires
an explicit decision and migration.

`ActorProvisioningService` consumes authorization evidence for exactly
`identity.manage`. It derives the provisioner actor from that evidence and
combines it with a host-assigned provisioning ID, stable actor ID, exact
authority and opaque subject, and trusted host time. Callers cannot supply the
trusted provisioner field directly.

One PostgreSQL transaction:

1. claims immutable provisioning evidence;
2. creates the stable internal actor; and
3. creates its exact initial external-identity mapping.

The audit insert uses `ON CONFLICT DO NOTHING` across all three unique
arbiters: provisioning ID, actor ID, and `(authority_id, subject)`. After a
conflict, a separate statement under PostgreSQL `READ COMMITTED` observes the
concurrent winner's committed state. Complete equality, including provisioner
and recorded time, is an exact retry and returns `AlreadyProvisioned`; any
different content is an identity collision.

Deferred foreign keys allow audit evidence to be the first write while
requiring the actor and exact three-column mapping to exist at commit. A
trigger rejects updates and deletes of provisioning evidence. Failure of any
dependent write rolls back the audit row, actor, and mapping together.

The operation creates no platform or tenant grants for the new actor.

## Consequences

An authorized platform operator can create a stable actor and its usable first
login identity as one auditable operation. Exact retries are safe, concurrent
exact commands have one winner, and reuse of any command identity for different
content fails without leaking the external subject in the domain error.

Installations migrated from ADR 0051 gain `identity.manage` only for the actor
recorded by the immutable bootstrap fact. Other platform operators are not
silently elevated. Fresh installations grant the bootstrap operator both
currently required actions.

Provisioning an actor grants no product access. Tenant membership, roles,
platform grant administration, additional external identities, identity
replacement, and deprovisioning remain separate authorized and audited
operations.

This slice supplies application and PostgreSQL boundaries only. It does not
add an HTTP route, CLI command, identity-provider integration, invitation flow,
or administration UI.

## Alternatives considered

Reusing `tenant.provision` was rejected because tenant creation does not imply
authority over platform identities and would make least-privilege grants
impossible.

Creating the actor first and attaching an identity later was rejected because
partial failure would leave an actor with no login path and no atomic audit
fact for the intended operation.

Granting default tenant or platform access was rejected because identity and
authorization have different lifecycles. Actor existence is never proof of
access.

Checking only the provisioning ID for idempotency was rejected because actor
ID and external identity are independently unique. Concurrent commands could
otherwise surface storage-specific constraint errors instead of one consistent
collision contract.
