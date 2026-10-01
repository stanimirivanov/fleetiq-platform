# Protected property-definition query

## TL;DR

Follow a property ID and version from an asset-type definition to read its
stable name, value kind, and optional canonical unit. The caller needs the
configured bearer credential and a current tenant `metadata.read` grant.

## HTTP contract

`GET /api/v1/tenants/{tenant_id}/properties/{property_id}/versions/{version}`
with `Authorization: Bearer <token>` returns:

```json
{
  "id": "thermal.temperature",
  "version": 1,
  "name": "Air temperature",
  "value_kind": "decimal",
  "canonical_unit": "Cel"
}
```

`value_kind` is `boolean`, `integer`, `decimal`, or `text`. The unit is `null`
when none was defined. Unit identifiers preserve their configured spelling;
this response does not convert measurements. Request the version in the
asset-type reference because the server does not select a latest version.

The version path component is a positive decimal integer within the `u32`
range. Invalid identifiers or versions return `400`; an absent exact version
returns `404`. Missing or rejected credentials return a generic `401` bearer
challenge, a denied current grant returns `403`, and operational failures
return a generic `500` problem. Responses include `Cache-Control: no-store`.

The route is absent without complete HTTP bearer identity configuration. The
secret-injection and TLS requirements in the
[asset query procedure](asset-query.md) apply unchanged. See
[ADR 0082](../adr/0082-read-exact-property-definitions.md).
