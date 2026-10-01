# Protected relationship-type definition query

## TL;DR

Read an exact relationship-type ID and version to obtain its stable,
operator-facing name. The caller needs the configured bearer credential and
a current tenant `metadata.read` grant.

## HTTP contract

`GET /api/v1/tenants/{tenant_id}/relationship-types/{type_id}/versions/{version}`
with `Authorization: Bearer <token>` returns:

```json
{
  "id": "generic.tows",
  "version": 2,
  "name": "Tows"
}
```

This describes the meaning of a directed relationship type, not an individual
edge or its allowed endpoints. Request the exact version recorded with the
relationship; the server does not select a latest version.

The version path component is a positive decimal integer within the `u32`
range. Invalid identifiers or versions return `400`; an absent exact version
returns `404`. Missing or rejected credentials return a generic `401` bearer
challenge, a denied current grant returns `403`, and operational failures
return a generic `500` problem. Responses include `Cache-Control: no-store`.

The route is absent without complete HTTP bearer identity configuration. The
secret-injection and TLS requirements in the
[asset query procedure](asset-query.md) apply unchanged. See
[ADR 0083](../adr/0083-read-exact-relationship-type-versions.md).
