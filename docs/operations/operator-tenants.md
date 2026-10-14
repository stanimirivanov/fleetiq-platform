# Operator tenant discovery

## TL;DR

GET /api/v1/me/tenants discovers the authenticated caller's active tenants and
exact actions. It supplies UI hints; every protected operation rechecks access.
The response never changes or extends the browser session.

## Request and response

Use the same-origin browser session cookie. Trusted workload installations may
use their configured bearer instead; the listener supports only one mode.
Never put the workload credential into browser code.

Optional query parameters are limit (1..100, default 50) and after (the exclusive
tenant ID from next_after). Unknown parameters and invalid IDs return 400 after
authentication. Clients cannot choose a different actor.

For example, GET /api/v1/me/tenants?limit=50 returns:

```json
{
  "tenants": [
    { "tenant_id": "tenant-a", "actions": ["metadata.read"] }
  ],
  "next_after": null
}
```

Tenants are ordered by ID. Actions are exact grants in ascending action-name
order: access.manage, metadata.manage, metadata.read. None implies another;
a tenant can have management access without metadata.read. Platform authority
does not imply tenant access. Tenant names are not currently persisted.

Follow next_after for later bounded pages; null means no later accessible
tenant was found in that request. A page limit applies to tenants, never
individual action rows. Do not display a page length as the total tenant count.

## Freshness, errors, and recovery

Each request reads current policy. Suspended tenants, inactive actors, other
actors' grants, and ungranted tenants are excluded. No grants returns 200 with
an empty array and null cursor. A previously returned cursor remains a position
if its tenant loses access. Separate pages are not a transactional snapshot:
concurrent changes may alter later pages or require restarting the traversal.

All responses use Cache-Control: no-store. Missing, expired, unmapped, or
inactive browser identity is 401. Workload mode alone emits a bearer challenge.
Malformed queries return 400; unavailable policy returns a bounded 500 problem,
not a successful empty result. The route is absent (404) without identity
configuration. Unsupported methods return 405.

Clear superseded UI hints on identity/tenant changes and refresh after denial.
An enabled control or previous discovery never authorizes a subsequent request.
Existing 403 responses from tenant operations remain authoritative.

## Compatibility and verification

The [modular contract](../../contracts/http/openapi/openapi.yaml) publishes
listOperatorTenants and bounded schemas. Consumers pin the exact merged
revision before generation. The session actor_id and expires_at_ms fields
remain unchanged. Rollback can remove this additive endpoint; handle unsupported
deployments explicitly rather than inferring grants.

PostgreSQL and HTTP tests cover policy freshness, pagination, actor isolation,
invalid queries, authentication, and denial after revocation. These fixtures do
not qualify an external identity provider or a deployed browser/TLS installation.
See [browser identity](browser-identity.md) and
[ADR 0099](../adr/0099-discover-current-operator-tenant-access.md).
