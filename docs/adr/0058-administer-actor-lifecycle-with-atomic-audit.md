# ADR 0058: Administer actor lifecycle with atomic audit

## Status

Accepted, 27 September 2026.

Supersedes: none. Superseded by: none.

## Context

FleetIQ provisions stable internal actors, maps externally authenticated
identities to them, and assigns exact tenant and platform grants. Removing an
actor row would destroy referential integrity for immutable provisioning and
administration evidence. Removing all mappings and grants would discard useful
configuration and make an accidental deprovisioning expensive to recover.

The system nevertheless needs an immediate, provider-neutral way to prevent a
human or workload from starting new authenticated operations. This transition
must be authorized, idempotent, auditable, and safe under concurrent commands.
It must also avoid disabling the final actors capable of recovering identity or
platform-access administration.

## Decision

Actors have an explicit active state. New actors are active. Deactivation keeps
the stable actor, external identities, grants, and historical references, but:

- exact external-identity resolution returns no actor; and
- current tenant and platform policy evaluations deny the actor.

Activation restores those paths using the retained configuration. Evidence
already produced before deactivation is not recalled; application evidence is
single-use, and later work may define cancellation for long-running operations.

Actor lifecycle administration consumes exact `identity.manage` platform
authorization. The application derives the administrator from that evidence
and accepts a host-assigned change identity, target actor, requested state, and
trusted time. PostgreSQL serializes all commands through the bootstrap
singleton, resolves exact retries, changes current state, and appends immutable
evidence in one transaction.

The audit row records the administrator, target, activation or deactivation,
recorded time, and whether current state changed. Reusing a change identity
with different content is a collision. A request for the current state records
`state_changed = false`; an exact retry returns the original effect.

Deactivation is rejected when the target is the final active holder of either
`identity.manage` or `platform.access.manage`. These checks run under the same
global lock as the state change. They preserve the two authorities needed to
reactivate actors and delegate platform access.

## Consequences

Actor identity and audit history remain stable while operational access can be
disabled and restored. Deactivation affects the next identity resolution and
the next current-policy evaluation, including callers whose authentication was
completed earlier but whose authorization has not yet occurred.

Grants and external mappings remain visible to authorized administration and
become effective again after reactivation. Operators must therefore change or
remove them separately when permanent access removal is intended.

The bootstrap singleton is the current global serialization anchor. This keeps
cross-actor recovery checks deterministic at the expected administration rate;
a future partitioned design would need another transactionally equivalent
lock strategy.

Actor lifecycle audit querying, bulk operations, scheduled suspension,
identity-provider synchronization, HTTP routes, and deletion or anonymization
policy remain separate work.

## Alternatives considered

Deleting the actor was rejected because immutable audit and provisioning facts
refer to its stable identity and must remain interpretable.

Deleting mappings and grants was rejected because it combines reversible
suspension with destructive configuration changes and complicates recovery.

Filtering only during external-identity resolution was rejected because typed
authenticated evidence can exist before deactivation. Current policy checks
also inspect active state to close that interval before authorization.

Allowing the final administrator to deactivate itself was rejected because it
could make lifecycle recovery or platform delegation impossible without direct
database intervention.
