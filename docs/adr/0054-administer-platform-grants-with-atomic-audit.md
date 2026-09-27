# ADR 0054: Administer platform grants with atomic audit

## Status

Accepted, 27 September 2026.

Supersedes: ADR 0053 for the bootstrap operator's initial platform grant set.
Superseded by: none.

## Context

FleetIQ can bootstrap one platform operator, provision additional actors, and
authorize exact platform actions from current PostgreSQL grants. It cannot yet
delegate those actions to another actor or revoke them through an authorized,
auditable operation. Direct grant-table changes would bypass application
authorization and could commit without evidence identifying the administrator,
request, trusted time, and resulting state effect.

Actor registration, tenant provisioning, and platform access administration
are different authorities. An operator allowed to create identities must not
implicitly be able to grant tenant creation or further platform administration
authority.

Grant changes are low-volume but security-sensitive. Requests may be retried
after an uncertain response and may arrive concurrently on different replicas.
The current grant and immutable audit evidence must therefore resolve as one
transactional command.

## Decision

Add the closed `platform.access.manage` platform action, represented by
`PlatformAction::ManagePlatformAccess`. It is evaluated as an exact grant and
is not implied by `identity.manage`, `tenant.provision`, or any tenant-scoped
action.

Migration 0017 explicitly grants `platform.access.manage` only to the actor
recorded in `identity.platform_bootstrap`. Fresh bootstrap grants all three
current installation authorities: `platform.access.manage`, `identity.manage`,
and `tenant.provision`. This supersedes ADR 0053 only where it specified the
bootstrap grant set. Future actions still require an explicit decision and
migration.

`PlatformAccessAdministration` consumes authorization evidence for exactly
`ManagePlatformAccess`. It derives the administrator actor from that evidence
and accepts a host-assigned platform change ID, target actor, exact platform
action, grant or revoke kind, and trusted host time.

The PostgreSQL adapter locks the immutable platform-bootstrap singleton before
reading or changing access. This serializes the low-volume global command
stream, including concurrent reuse of one platform-scoped change ID for
different target actors. Inside the same transaction it:

1. resolves an existing audit record as an exact retry or identity collision;
2. inserts or deletes the exact current grant; and
3. appends immutable audit evidence with whether grant state changed.

Grant is ensure-present and revoke is ensure-absent. A new command is recorded
even when the requested state already holds, with `state_changed = false`.
Retrying the same change ID requires complete equality of administrator,
target, action, kind, and recorded time and returns the original effect.

The audit table has foreign keys to both actors, a closed action and operation
vocabulary, and a trigger rejecting update and deletion. Failure of the grant
mutation or audit insert rolls back both. Revoking the administrator's own
grant is allowed using already-consumed authorization evidence and affects the
next policy decision, provided another `platform.access.manage` grant remains.
The repository rejects removal of the final platform access administrator
under the same global lock and rolls back that command without an audit row.

## Consequences

The bootstrap operator can delegate or revoke exact platform capabilities
without direct database editing. Current-state authorization sees each
committed change on the next decision, while immutable evidence explains who
requested it and whether it changed state.

The distinct administration action preserves least privilege. An identity
operator or tenant provisioner cannot administer platform grants unless it has
the additional exact grant. Delegation of `platform.access.manage` is explicit
and auditable, and the final holder cannot accidentally lock the installation
out of further administration.

All platform grant changes serialize on one row. This is appropriate for the
expected low command volume and gives global change IDs deterministic meaning.
If measured administration throughput later requires partitioning, the lock
and identity scope must be replaced together without weakening collision
handling.

The application and persistence boundary has no HTTP route, CLI command,
approval workflow, audit query API, or administration UI. Those remain
separate capabilities.

## Alternatives considered

Using `identity.manage` was rejected because creating login identities does not
imply authority to delegate tenant provisioning or access administration.

Mutating the grant table without an audit row was rejected because partial
failure or direct retries could leave unexplained authority changes.

Locking only the target actor was rejected because a platform-scoped change ID
could race across two different target actors. One global lock keeps both grant
state and command identity deterministic.

Inserting audit evidence before the mutation was rejected because the durable
record includes whether current grant state changed. Computing and storing that
effect in one serialized transaction avoids later audit updates.
