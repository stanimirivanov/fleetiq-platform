# Asset and communication-device registration

## TL;DR

Register an asset first, then optionally attach communication capability. Use the
same internal identity and complete creation content when retrying an uncertain
response. First creation returns 201; an exact retry returns 200 with the original
actor/time. A conflict never updates an object.

## Authority and prerequisites

Apply migrations through 0029. Configure one existing browser-session or workload
identity mode. Publish the exact asset-type version in the selected tenant using
[definition commissioning](semantic-definitions.md). The operator needs exact
current `metadata.manage` for that active tenant. No platform or other tenant
grant substitutes; `metadata.read` is independent.

For browser POSTs, send the session cookie, one exact configured `Origin` and
`X-FleetIQ-CSRF: 1`. Never embed workload credentials in browser code. The server
derives tenant scope from authorization and actor/time from trusted host context.

## Register an asset

POST `/api/v1/tenants/{tenant_id}/assets` with `application/json`:

```json
{
  "id": "asset-a",
  "name": "Machine",
  "asset_type": { "id": "equipment", "version": 1 },
  "external_identifiers": [
    { "kind": "imei", "authority": null, "value": "350000000000001" },
    { "kind": "manufacturer.serial", "authority": "maker-a", "value": "SN-100" }
  ]
}
```

The type version must already exist in this tenant. IDs are internal opaque values;
no external scheme is universal. Names are labels. Identifier kind, optional
authority and value form a tenant-owned external namespace. Omitted authority
means null. Identifier order does not affect retry equality; duplicates are invalid.
The required array can be empty and contains at most 100 entries.

## Attach communication capability

POST `/api/v1/tenants/{tenant_id}/assets/{asset_id}/device`:

```json
{
  "endpoints": [
    { "id": "onboard", "kind": "device.onboard", "name": "Onboard measurements" },
    { "id": "can-primary", "kind": "bus.can", "name": "Primary CAN interface" }
  ]
}
```

The same-tenant asset must already exist. This creates capability on its existing
identity, rather than another asset. Up to 100 endpoints are allowed, including
an empty declaration. Endpoint IDs must be unique in the device. Order, ID, kind
and label all participate in equality. Reordering or editing endpoints is a
conflict; this is not an endpoint update API.

This operation neither chooses a decoder nor activates a protocol package, nor
attributes observations to another asset. Those remain separate capabilities.

## Outcomes, evidence and recovery

| Status | Meaning | Operator action |
|---|---|---|
| 201 | Complete new registration and evidence committed | Retain returned identity/evidence. |
| 200 | Equal content already registered | Retain original evidence; no new creation occurred. |
| 400 | Invalid, unknown, duplicate or oversized input | Correct input; no partial aggregate remains. |
| 401 | Caller cannot authenticate | Restore the selected identity/session. |
| 403 | Exact current tenant authority or browser guard missing | Check tenant state, grants and Origin/CSRF. |
| 404 | Referenced exact type or underlying asset absent in this tenant | Commission it first; foreign data cannot satisfy the reference. |
| 405 | Unsupported method | Use the advertised GET, HEAD or POST. |
| 409 | Different content, occupied external identity or unattributed legacy object | Inspect existing data; never blindly choose a new key after uncertainty. |
| 500 / lost response | Storage failure or ambiguous commit | Reauthenticate if necessary and retry identical key/content. |

A 201 response contains `outcome: registered`, `registered_by` and
`recorded_at_ms` (signed Unix milliseconds). A 200 response contains
`already_registered` and exactly the original actor/time, even when another
authorized operator retries. Do not send these context fields in a request.

The aggregate, identifiers/endpoints and evidence are transactional. No partial
parent survives an identifier, reference or audit failure. Evidence rejects SQL
UPDATE, DELETE and TRUNCATE. Pre-existing objects created by strict internal
insert paths have unknown attribution; matching HTTP requests return 409 rather
than inventing their provenance. Resolve such legacy commissioning through an
explicit future migration procedure, not a fabricated registration record.

All responses are no-store; public problems omit database constraints and
credentials. JSON bodies are limited to 32 KiB with closed nested shapes.
Identifiers accept at most 128 Unicode scalar values, names at most 200, with
no surrounding whitespace or control characters.

## Read and diagnose

Use the unchanged exact asset GET and
[communication-device GET](device-query.md) with exact `metadata.read`.
The device GET returns `tenant_id`, `asset_id` and the declared ordered
`endpoints`. Inspect registration evidence in
`metadata.registration_records` through authorized operational database access;
this slice adds no audit search endpoint.

Healthy commissioning yields one evidence row per tenant/asset/registration-kind.
Exact retries must not add rows or change attribution. Investigate unexpected
409s against the original complete content and tenant namespace before changing
IDs. Infrastructure failures require checking database readiness and migration
state; do not expose raw SQL errors to operators.

Rollback uses a follow-up build that removes the additive command routes while
retaining migration 0029, registered objects and evidence. The server applies its
embedded migration chain before binding its listener; SQLx rejects a database
migration absent from that binary's inventory. A pre-0029 binary therefore cannot
simply restart against the upgraded database. Do not delete evidence, rewind the
migration ledger or disable missing-migration validation to enable rollback.
The follow-up build keeps existing reads available. No backfill or data deletion
is part of rollback.
