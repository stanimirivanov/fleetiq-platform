# ADR 0060: Administer tenant lifecycle with atomic audit

## Status

Accepted, 28 September 2026.

Supersedes: none. Superseded by: none.

## Context

FleetIQ can provision stable tenant ownership boundaries and exact tenant
grants, but it cannot suspend a tenant without deleting data or individually
revoking every grant. Deletion would break durable references and is unsuitable
for temporary operational, commercial, or incident-response restrictions.

Tenant provisioning authority should not imply authority to suspend an
existing customer boundary. Lifecycle commands also need retry-safe identity,
trusted administrator attribution, and immutable evidence.

## Decision

Tenants have an explicit active state. New and existing tenants are active.
Suspension retains the tenant row, owned metadata, action grants, and historical
references while making fresh tenant authorization policy decisions deny the
tenant. Activation restores authorization through the retained grants.

Add the exact `tenant.lifecycle.manage` platform action, represented by
`PlatformAction::ManageTenantLifecycle`. It is distinct from tenant
provisioning, identity administration, platform grant administration, and all
tenant-scoped actions. Migration 0023 grants it to the immutable bootstrap
operator; later delegation uses audited platform-access administration.

Tenant lifecycle administration consumes that exact authorization evidence.
The application derives the administrator from the evidence and accepts a
host-assigned change identity, target tenant, activate or suspend operation,
and trusted time.

PostgreSQL serializes commands through the bootstrap singleton. One transaction
resolves an exact retry, reads current tenant state, changes it when necessary,
and appends immutable evidence. Reusing a change identity with different
content is a collision. A request for the existing state records
`state_changed = false`.

The audit row records the change identity, administrator, tenant, operation,
trusted time, and state effect. It cannot be updated or deleted.

## Consequences

Operators can suspend access to a tenant without discarding customer data or
authorization configuration. Reactivation is explicit and restores retained
grants. Deactivation of an actor and suspension of a tenant remain independent
policy dimensions; either condition denies fresh tenant authorization.

Previously issued typed authorization evidence is not recalled. The current
metadata application services consume that evidence, so work already
authorized may complete. Long-running worker cancellation requires a separate
design.

The current ingestion path is not yet composed into a production worker and
does not consult tenant authorization. Before production ingestion is enabled,
session admission and durable workers must define how suspended tenants stop,
drain, reject, or quarantine traffic.

The bootstrap singleton is the current global serialization anchor. This is
appropriate for low-rate administration and platform-scoped command identity;
a future partitioned design needs equivalent collision semantics.

Tenant lifecycle audit querying, deletion and anonymization, scheduled
suspension, billing integration, HTTP routes, and user interface remain
separate work.

## Alternatives considered

Deleting tenant data was rejected because suspension is reversible and durable
references must remain interpretable.

Deleting tenant grants was rejected because it turns suspension into a
destructive policy rewrite and complicates safe reactivation.

Reusing `tenant.provision` was rejected because creating a new boundary does
not imply authority to disable an existing customer boundary.

Checking tenant state only in application handlers was rejected because the
shared policy adapter is the narrowest current boundary for every fresh tenant
authorization decision.
