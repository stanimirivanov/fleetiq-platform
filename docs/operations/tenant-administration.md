# Tenant provisioning and lifecycle administration

## TL;DR

Provision tenants with exact platform `tenant.provision`. Catalogue, activation,
suspension and lifecycle audit require distinct `tenant.lifecycle.manage`.
Browser writes also require the configured Origin and `X-FleetIQ-CSRF: 1`.
Keep command identities and business fields stable after uncertain results.

## Authentication and authority

Routes exist only on an identity-configured listener. Use same-origin browser
sessions or the separately configured workload bearer mode. Never expose a
workload credential to browser JavaScript. Every request rechecks active actor
identity and exact current permission, including retries. Tenant grants and other
platform actions cannot authorize these operations. No new configuration is needed.

## Provision an active tenant

POST `/api/v1/admin/tenants`:

```json
{"provisioning_id":"provision-tenant-a-001","tenant_id":"tenant-a","initial_administrator_actor_id":"admin-a"}
```

Select an existing internal actor. The transaction creates the tenant, immutable
provisioning evidence and exactly `metadata.read`, `metadata.manage` and
`access.manage` grants for that actor. The provisioner gets no implicit tenant
grant or platform delegation. Choose an active administrator and verify their
discovery and read access before handover. An inactive actor may retain grants
but cannot exercise them until activated through actor administration.

First success is HTTP 201 with `{"outcome":"provisioned"}`; an exact delivery retry
returns 200 with `{"outcome":"already_provisioned"}`. A missing initial actor
returns 404 and rolls back everything. Conflicting command or tenant identity
returns 409. Replaying provisioning does not restore revoked grants or reactivate
a subsequently suspended tenant.

## Activate or suspend

POST `/api/v1/admin/tenants/tenant-a/lifecycle`:

```json
{"change_id":"suspend-tenant-a-001","state":"suspended"}
```

Use `active` to activate. HTTP 200 returns `outcome` (`recorded` or
`already_recorded`) and original `state_changed`. A fresh command for an already
satisfied state records a no-change fact. Missing tenants return 404. An old
command never undoes a later transition.

Suspension denies fresh tenant authorization and removes the tenant from operator
discovery. Data, grants and audit remain. Platform catalogue and lifecycle
administration remain available for recovery. Suspension does not cancel
in-flight work, recall issued authorization or stop ingestion and background
workers; those runtime controls are outside this capability.

## Catalogue and immutable lifecycle audit

GET `/api/v1/admin/tenants?limit=50` returns `tenants: [{tenant_id,state}]` and
nullable `next_after`. Pass that exact tenant ID as `after` to continue ascending
ID order. Both active and suspended tenants appear; this catalogue is platform
administration, not the caller's accessible-tenant discovery.

GET `/api/v1/admin/tenant-lifecycle-changes?limit=50` returns `changes` and nullable
`next_before`. Each fact contains change ID, trusted administrator, tenant,
activate/suspend kind, original recorded time and original state-change effect.
Continue with both `before_recorded_at_ms` and `before_change_id`, or neither.
Ordering is descending time then change ID, including tied times. Limits default
to 50 and must be 1..100. Pages reflect committed state without a traversal snapshot.
Initial provisioning evidence remains separate from lifecycle history.

## Retry, failures and recovery

The server derives administrator and receipt time. Keep each command ID, tenant,
initial administrator or requested state unchanged after a lost response. Retries
retain the first durable time and original result; different content or
authenticated administrator conflicts. Fresh current authority is still required.
Another authorized platform operator must inspect current state and audit when the
original caller loses authority after an uncertain response.

Unknown fields, invalid identifiers, unsupported state and bodies over 16 KiB
return safe 400 problems. Authentication and authorization precede input diagnostics.
All outcomes are no-store; storage errors are generic and never disclose SQL,
constraints or credentials. Unsupported methods advertise exact Allow headers.

Rollback removes additive routes and retains tenant data, grants and immutable
audit. Deployed provider, HTTPS-edge and production worker qualification are
separate. See [ADR 0103](../adr/0103-expose-tenant-administration-over-http.md).
