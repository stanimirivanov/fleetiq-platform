# Protected asset-type definition query

## TL;DR

Use an asset's `asset_type.id` and `asset_type.version` to retrieve its pinned
definition. The caller needs the same configured bearer credential and current
tenant `metadata.read` grant as the asset and device queries.

## HTTP contract

`GET /api/v1/tenants/{tenant_id}/asset-types/{type_id}/versions/{version}`
with `Authorization: Bearer <token>` returns:

```json
{
  "id": "generic.power-system",
  "version": 2,
  "name": "Power system",
  "supported_properties": [
    { "id": "thermal.temperature", "version": 1 },
    { "id": "electrical.voltage", "version": 3 }
  ]
}
```

Property references retain their definition order. They identify immutable
property versions; this response does not include their names, value kinds,
or units. Follow a reference with the
[property-definition query](property-query.md). An empty array is valid. A
client must request the exact version from the asset; the server does not
select a latest version.

The version path component is a positive decimal integer within the `u32`
range. Invalid identifiers or versions return `400`. An absent exact
definition returns `404`. Missing or rejected credentials return the generic
`401` bearer challenge, a denied current grant returns `403`, and operational
failures return a generic `500` problem. Responses include
`Cache-Control: no-store`.

The route is absent without complete HTTP bearer identity configuration. The
secret-injection and TLS requirements in the
[asset query procedure](asset-query.md) apply unchanged. See
[ADR 0081](../adr/0081-read-exact-asset-type-versions.md).
