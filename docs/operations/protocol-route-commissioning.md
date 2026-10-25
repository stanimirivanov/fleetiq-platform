# Protocol route commissioning

## TL;DR

Commission an exact admitted release for a registered device/channel using
`protocol.manage`. GET current configuration, PUT an explicit selection or null
retirement with its expected revision, then refresh GET. This configures routing;
it does not open a listener or authenticate a network peer.

## Prerequisites

- Apply migrations through 0032. The application role needs SELECT/INSERT/UPDATE
  on route heads, SELECT/INSERT on route history and the existing catalogue,
  artifact, admission and metadata read privileges. Do not grant history UPDATE,
  DELETE or DDL authority; immutable triggers and deferred integrity remain active.
  The serving role also needs its existing tenant-administration UPDATE privilege
  on at least one `metadata.tenants` column for the tenant `FOR SHARE` lock;
  [PostgreSQL documents this locking privilege](https://www.postgresql.org/docs/18/sql-select.html).
- Register the asset, its communication capability and exact external identifier
  with the correct issuer namespace. Identifiers never create devices implicitly.
- Store and explicitly admit the selected exact package version/world under the
  current policy; see [artifact admission](protocol-package-admission.md).
- Delegate current tenant `protocol.manage` through the audited access API.
- Choose a host channel name: 1–128 ASCII letters/digits, dots, underscores or
  hyphens. A channel name is configuration identity, not a TCP port or listener.

## Read current configuration

`GET /api/v1/tenants/{tenant_id}/assets/{asset_id}/protocol-routes/{channel_id}`

A missing configuration returns 404. An existing configuration returns its current
revision, explicit selection or null retirement, pinned policy, original actor
and receipt time. A GET is configuration inspection, not a fresh runtime
eligibility or authentication result. Protected responses are no-store.

## Select an exact admitted release

PUT on the same route with `Content-Type: application/json` and a body up to 4 KiB:

```json
{
  "expected_revision": 0,
  "selection": {
    "package_id": "custom.binary",
    "package_version": "1",
    "world": "identity-v1"
  }
}
```

Use zero only when no configuration exists; otherwise use the GET revision.
Choose `runtime-v1` or `identity-v1` explicitly. Both must have their own successful
admission evidence. Browser mutations require the configured exact Origin and
`X-FleetIQ-CSRF: 1`; workload credentials remain server-side.

Success returns 200 and the durable command receipt. Same-actor/content delivery
retries preserve the first generation/actor/time, including after later changes.
They never restore an old head. Refresh GET before making another change.
A new command using the current generation creates the next generation even
when selecting identical content; older session handoffs become invalid.

409 means a competing revision, exhausted revision range, receipt-time regression
or an unavailable device/content/admission prerequisite. Inspect current state
and prerequisites before retrying; do not blindly change expected_revision.
400 rejects invalid/oversized bodies and omitted selection. Authentication and
current authorization precede request diagnostics; 401/403 never imply existence.

## Retire one route

PUT an explicit null selection and the current revision:

```json
{"expected_revision": 1, "selection": null}
```

Omission is invalid. Retirement appends history and makes this device/channel
ineligible; it does not delete component bytes, admission evidence or other
routes using that package. Restore service by commissioning another generation.

## Trusted host handoff

The ingestion service exposes internal `resolve` and `revalidate` operations.
A future listener chooses `RegisteredIdentityChannel`: tenant, channel, identifier
issuer (or explicit implicit issuer), exact package/world. Guest claims supply
only bounded kind/value evidence. There is no public session-impersonation API,
global tenant search, implicit registration or inferred cryptographic identity.

Lookup requires a current active tenant, exact route generation, registered
identifier/device, stored content and current admission. Pin its content and
route generation, then revalidate immediately before each bounded transport turn.
Any denial or repository failure requires closing/discarding the handoff; do not
resume a denied handoff after recovery. Changed routes remain invalid even when
identical content is selected again. Suspension denies validation while inactive;
this slice does not track a tenant lifecycle epoch or push socket cancellation.
Work already started under a valid snapshot may finish within its existing bound.

Revoking an operator grant prevents new commissioning commands; it does not
retire previously commissioned devices. Explicit route retirement controls those
devices independently of the operator account lifecycle.

No transport is enabled by these HTTP commands. Listener startup, authentication
and shutdown integration are subsequent delivery-plan work. Preserve route tables
in database backups; do not delete history or rewrite heads to recover service.
See [ADR 0113](../adr/0113-activate-revisioned-device-protocol-routes.md).
