# Tenant-access administration

## TL;DR

Use the authenticated HTTP listener to grant or revoke an existing actor's exact
tenant action, inspect that actor's current grants, and read tenant access audit.
Every operation requires current access.manage in the selected active tenant.
Platform administration grants do not give tenant access.

## Prerequisites and authority

Configure either [browser sessions](browser-identity.md) or the separate
workload authentication mode. Provision the tenant and its first administrator
using the [tenant procedure](tenant-administration.md); provision target actors
using the [actor procedure](actor-administration.md).

Permissions are exact: metadata.read, metadata.manage and access.manage do not
imply one another. A tenant access administrator can manage all three actions
within that tenant, but cannot provision actors or alter platform grants through
these routes. Suspended tenants deny these operations. Inactive target actors
may retain grants but cannot exercise them.

For a browser mutation, send the session cookie, exactly one configured public
Origin and exactly one X-FleetIQ-CSRF header with value 1. The selected browser
listener rejects bearer credentials instead of using them as fallback.

## Commands and retries

POST /api/v1/tenants/{tenant_id}/actors/{actor_id}/access-changes accepts:

~~~json
{"change_id":"grant-reader-1","kind":"grant","action":"metadata.read"}
~~~

Use kind revoke to remove that exact grant. Unknown fields are rejected and JSON
is capped at 16 KiB. The route selects the tenant and target actor; the host
derives the authenticated administrator and receipt time. Callers cannot supply
audit attribution or time.

A successful command returns HTTP 200 with outcome recorded or already_recorded
and state_changed true or false. A new already-satisfied grant/revoke is still
audited. Keep the same ID, target, action and kind after an ambiguous response.
IDs are unique within a tenant, so separate tenants can use the same value.
Do not generate a new ID merely because the response was lost.

A retry retains the original committed time and effect and never replays the
mutation over later changes. Reusing the ID with different content or another
administrator returns 409. Correcting intent requires a new ID. Missing target
actors return 404; a failed command commits neither grant changes nor audit.

## Current grants and audit

GET /api/v1/tenants/{tenant_id}/actors/{actor_id}/grants returns grants and
next_after. Grants are ordered by exact action name. Use limit (1..100, default
50) and after_action from next_after to continue. A removed cursor grant need
not still exist. Missing actors and actors with no grants both return an empty
terminal page; this resource is not an actor-existence check.

GET /api/v1/tenants/{tenant_id}/access-changes returns changes and next_before,
newest first by (recorded_at_ms, change_id). Continue with both
before_recorded_at_ms and before_change_id from next_before, plus the bounded
limit. Each entry includes tenant, administrator, target, action, kind, receipt
time and original state_changed. Unknown or duplicate query fields are invalid.

Cursors are traversal positions, not authorization or consistent snapshots
across requests. Fresh authority is checked on every page. Initial grants from
tenant provisioning have separate evidence; do not reconstruct all current
grants from access-change history alone.

## Delegation, lockout and suspension

Delegate access.manage to another active, correctly mapped actor and verify that
actor can read these resources before revoking your own access. Existing tenant
policy permits final-administrator self-revocation. There is no tenant lockout
prevention, platform override or recovery API in this release. A platform grant
alone cannot repair tenant grants through this endpoint; unresolved lockout
requires a separately reviewed operational intervention.

Self-revocation takes effect at the next authorization decision. Even an exact
retry can then return 403. Another authorized administrator can inspect the audit,
but cannot reuse the original administrator's command ID as their own command.
Suspension also denies these reads and writes while retaining grants and audit.
Platform tenant-lifecycle authority can reactivate a suspended tenant; activation
does not restore revoked grants. Issued evidence and in-flight work are not recalled.

## Failures and rollback

All outcomes are no-store. Problems expose safe status meanings without database
details: 400 invalid actor/command/query, 401 authentication required, 403 missing
tenant authority or failed browser guard, 404 missing mutation target, 405 wrong
method, 409 command collision, and 500 infrastructure failure. An unknown,
invalid or inaccessible tenant returns 403 rather than disclosing its existence.

No migration or new runtime configuration is required. Rollback removes the
additive HTTP surface and retains stored grants, audit and exact command behavior.
The [modular contract](../../contracts/http/openapi/openapi.yaml) defines wire shapes.
