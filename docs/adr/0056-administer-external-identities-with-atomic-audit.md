# ADR 0056: Administer external identities with atomic audit

## Status

Accepted, 27 September 2026.

Supersedes: ADR 0051 and ADR 0053 only where their foreign keys permanently
retain an actor's initial external-identity mapping.
Superseded by: none.

## Context

FleetIQ can atomically provision an actor with its first external identity and
resolve an authenticated authority and subject to that stable actor. It cannot
attach a recovery or replacement identity, detach a retired identity, or
record who performed either operation. Direct table changes could reassign a
principal, remove an actor's only login path, or commit without immutable audit
evidence.

Bootstrap and actor-provisioning evidence currently has a deferred foreign key
to the exact current mapping. That proves the mapping exists when provisioning
commits, but it also pins the initial mapping forever. Identity-provider
migration requires historical evidence to remain immutable while current login
identities evolve.

Identity commands are security-sensitive, may be retried after an uncertain
response, and can arrive concurrently on different replicas. A platform-scoped
change identity must keep one meaning even when requests target different
actors.

## Decision

`ExternalIdentityAdministration` consumes authorization evidence for exactly
`ManageIdentity`. It derives the administrator actor from that evidence and
accepts a host-assigned change ID, target actor, exact trusted authority and
opaque subject, attach or detach kind, and trusted host time.

The PostgreSQL adapter locks the immutable platform-bootstrap singleton. This
serializes the low-volume global identity command stream, makes platform-scoped
change IDs deterministic, and prevents concurrent detach commands from each
observing another identity before jointly removing an actor's final mappings.
Inside one transaction it:

1. resolves prior audit evidence as an exact retry or identity collision;
2. ensures the requested current mapping state;
3. rejects an identity already assigned to another actor;
4. rejects removal of the target actor's final external identity; and
5. appends immutable audit evidence including whether state changed.

Attach is ensure-present for the exact target actor. Detach is ensure-absent
for that actor. A new command is recorded with `state_changed = false` when the
requested state already holds. An identity assigned to another actor is a
conflict for both operations rather than an absence, preventing misleading
audit evidence. Routine errors omit the opaque external subject.

The provisioning foreign keys to current mappings are replaced by deferred
constraint triggers that run only when new bootstrap or actor-provisioning
evidence is inserted. Provisioning still cannot commit without its exact
initial mapping. A later authorized detach no longer invalidates or rewrites
the historical provisioning fact. Actor foreign keys and audit immutability
remain unchanged.

## Consequences

An identity administrator can attach a recovery or replacement login and then
retire an earlier provider identity without changing the stable internal actor
or its grants. Exact retries return the recorded effect, changed-content ID
reuse is rejected, current mapping and audit commit atomically, and an actor
always retains at least one external login identity through this boundary.

All identity changes serialize on one row. This is appropriate for expected
administrative volume and matches the global change-ID scope. If measured
throughput later requires partitioning, the lock, change-ID scope, and final-
identity check must evolve together.

The final-identity rule applies to this administration boundary. Future actor
deprovisioning needs a separate authorized transaction that removes grants and
the actor under explicit lifecycle semantics. Database-owner intervention can
still bypass application repositories and remains an operationally privileged
action.

This slice adds application and PostgreSQL boundaries only. It does not add an
HTTP route, identity-provider adapter, invitation or approval workflow, audit
query, deprovisioning operation, or administration UI.

## Alternatives considered

Keeping the permanent provisioning foreign keys was rejected because identity
replacement would remain impossible without deleting immutable evidence.

Dropping those foreign keys without an insertion assertion was rejected
because direct or defective provisioning writes could commit evidence without
the exact initial mapping they claim to have created.

Automatically moving an identity between actors was rejected because it could
transfer access across stable security principals. An operator must detach and
attach through explicit commands, and detachment cannot leave an actor without
a login identity.

Allowing removal of the final identity was rejected because ordinary identity
maintenance should not silently create an inaccessible actor. Actor retirement
needs broader grant and lifecycle semantics.
