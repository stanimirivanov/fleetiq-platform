# Protected communication-device query

## TL;DR

Use the same configured bearer identity and tenant `metadata.read` grant as
the [asset query](asset-query.md). A device is an asset with communication
capability; its endpoint declarations are returned in configured order.

## Preconditions and response

The configured credential must resolve to an active actor with a current
`metadata.read` grant for the selected active tenant. The selected asset must
have communication capability registered through the metadata application
service. Endpoint kinds and names come from that device declaration, not from
vendor telemetry or semantic signal bindings.

`GET /api/v1/tenants/{tenant_id}/assets/{asset_id}/device` with
`Authorization: Bearer <token>` returns:

```json
{
  "tenant_id": "tenant-a",
  "asset_id": "gateway-1",
  "endpoints": [
    { "id": "onboard", "kind": "device.onboard", "name": "Onboard measurements" },
    { "id": "can-primary", "kind": "bus.can", "name": "Primary CAN interface" }
  ]
}
```

An empty `endpoints` array is valid for a registered device. Missing or
unregistered credentials return the same generic `401` challenge. A denied
current grant returns `403`; invalid identifiers return `400`; an asset with
no communication capability returns `404`. Repository failures return a
generic `500` problem. Responses carry `Cache-Control: no-store`.

The route is absent unless all three HTTP bearer identity variables are
configured. The credential and TLS requirements in the
[asset-query procedure](asset-query.md) apply unchanged. See
[ADR 0080](../adr/0080-expose-communication-device-detail.md) for the resource
and security decision.
