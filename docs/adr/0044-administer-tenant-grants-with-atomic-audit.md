# ADR 0044: Administer tenant grants with atomic audit

## Status

Accepted, 18 September 2026.

Supersedes: none. Superseded by: none.

## Context

[ADR 0043](0043-persist-exact-tenant-action-grants.md) established current
tenant action grants and a read-only policy adapter, but deliberately left grant
mutation without a supported product path. Access changes are privileged
operations. FleetIQ must attribute each supported mutation, make retries safe,
and prevent current policy from changing without matching durable audit
evidence.

Transport retries can occur after an ambiguous response, and two administrators
can act concurrently. A retry must not create duplicate audit evidence, while a
reused command identity must not silently refer to different actors, actions,
or operations.

## Decision

The application `TenantAccessAdministration` service consumes one
`AuthorizedTenantAction` whose exact action is `ManageAccess`. It derives tenant
and administrator identity from that authorization evidence. Callers supply
only a host-assigned change identity, target actor, target action, grant or
revoke kind, and trusted host recorded time.

Each command has a `TenantAccessChangeId` scoped by tenant. The PostgreSQL
repository records current grant state and an immutable row in
`identity.tenant_access_changes` in one transaction. The audit row contains
what happened, when it was accepted, the tenant, the authenticated
administrator, the affected actor and action, and whether current state changed.
Commands that request an already-held state are still recorded with
`state_changed = false`.

An exact retry returns the effect stored by the original command without
mutating state or appending another row. Reusing the same tenant-scoped change
identity for different command content is an identity collision. Audit rows
reject update and delete; corrections and compensating actions receive new
change identities.

The repository locks the stable tenant row before resolving the command. This
serializes access changes within one tenant, including commands with the same
identity but different target actors. Access administration is expected to be
low volume, so simple deterministic concurrency semantics are preferred over
more granular locking. Tenants remain independent.

Authorization rejection occurs before persistence. Database failures roll back
both the current-state mutation and audit insert. Separate security logging for
authorization failures and infrastructure failures remains an operational
observability concern; a failed transaction cannot safely claim a durable audit
record in the same database.

## Consequences

Application code now has one supported path for changing grants, and each
successful or no-op command has attributable immutable evidence. Consuming the
authorization value prevents ordinary callers from applying several changes
with one authorization result. Exact retry behavior makes ambiguous responses
safe to retry.

Per-tenant serialization limits concurrent access administration throughput for
one tenant. This is acceptable for infrequent privileged changes and can be
revisited with a stable finer-grained lock identity if measured demand requires
it.

The database cannot prevent a privileged operator from changing the current
grant table directly. Production database permissions, operational audit, and
break-glass procedures must restrict and observe that path. Product APIs must
use the application service.

The slice does not provision actors, expose an API, select an identity provider,
or provide audit search and retention. Those concerns remain separate work.

## Alternatives considered

Writing a best-effort log after changing the grant was rejected because a crash
could leave authorization state without audit evidence.

Using timestamps as idempotency keys was rejected because clocks do not provide
stable command identity and simultaneous changes are valid.

Recording only state-changing commands was rejected because an administrator's
attempt to establish an already-held state is still relevant audit evidence and
must have a deterministic retry result.

Locking only an existing grant row was rejected because grants can be absent and
grant and revoke commands would then have different concurrency behavior.

Implementing mutation as direct HTTP or SQL handler logic was rejected because
authorization, trusted audit attribution, idempotency, and atomicity belong to
the application and persistence boundaries rather than a transport.
