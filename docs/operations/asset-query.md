# Protected asset query

## TL;DR

Set all three HTTP identity environment values to enable exact asset and
bounded catalogue reads. Provision the exact external identity and an active
actor with a current `metadata.read` grant for the selected tenant. Keep the
credential in approved secret injection, and use TLS termination outside a
loopback deployment.

## Configuration

| Variable | Meaning |
|:--|:--|
| `FLEETIQ_HTTP_BEARER_TOKEN` | Random 32-byte secret encoded as exactly 64 hexadecimal characters. |
| `FLEETIQ_HTTP_IDENTITY_AUTHORITY` | Trusted stable authentication-authority ID for this credential. |
| `FLEETIQ_HTTP_IDENTITY_SUBJECT` | Opaque subject that the credential identifies within that authority. |

All three variables are optional together. Without them the routes are not
mounted. Setting only some fails startup. The credential and subject are
redacted from configuration debug output. Do not put the token in a command
argument, committed file, URL, log, or screenshot. Generate it with a
cryptographically secure random source; use your deployment secret manager for
injection and rotation. Replacing the credential requires a process restart.

The configured authority and subject must already map to an active actor. The
actor needs a current `metadata.read` grant for an active tenant. The requested
asset must exist in that tenant and reference a published asset-type version.
The existing platform bootstrap and authorized provisioning services establish
these records; an HTTP administration API is not part of this slice.

## HTTP contract

`GET /api/v1/tenants/{tenant_id}/assets/{asset_id}` with
`Authorization: Bearer <token>` returns a JSON asset document:

```json
{
  "id": "asset-1",
  "tenant_id": "tenant-a",
  "name": "Primary machine",
  "asset_type": { "id": "generic.machine", "version": 1 },
  "external_identifiers": []
}
```

`GET /api/v1/tenants/{tenant_id}/assets?limit=50` returns a bounded catalogue
of asset summaries:

```json
{
  "assets": [
    {
      "id": "asset-1",
      "tenant_id": "tenant-a",
      "name": "Primary machine",
      "asset_type": { "id": "generic.machine", "version": 1 }
    }
  ],
  "next_after": null
}
```

`limit` defaults to 50 and accepts 1 through 100. Assets are ordered by
stable ID. When `next_after` is not `null`, request the next page using
`after=<next_after>` and the same limit. An empty catalogue returns `200` with
an empty `assets` array. Invalid limit, cursor, or query fields return `400`.
Use the exact asset route to retrieve external identifiers; their per-asset
count is not bounded by the catalogue page limit.
The route uses the same credential, current tenant grant, error handling, and
no-store policy as the exact asset query. New registrations can appear between
pages; restart traversal if a complete current catalogue is required. See
[ADR 0088](../adr/0088-page-tenant-asset-catalogue.md).

External identifiers include `kind`, nullable `authority`, and `value`.
Clients must treat them as tenant-owned metadata. The server sends
`Cache-Control: no-store` for protected responses. Missing or rejected
credentials, including valid credentials without an actor mapping, return the
same `401` problem and `WWW-Authenticate: Bearer realm="FleetIQ"` challenge.
Denied current policy returns `403`; invalid identifiers return `400`; an
authorized missing asset returns `404`. Operational failures return a generic
`500` problem without internal details.

The default server listener is loopback. Any deployment that exposes this route
outside the host must terminate TLS at a trusted boundary, restrict access to
the secret, and prevent bearer headers from being logged. This single-principal
credential is a first query adapter, not a workforce sign-in or a production
identity-provider integration. See [ADR 0079](../adr/0079-protect-initial-asset-query.md).
