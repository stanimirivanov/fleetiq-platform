# Protected signal-binding revision query

## TL;DR

Read one exact recorded signal-binding revision with a configured bearer
credential and current tenant `metadata.read` grant. It explains an attribution
fact, not necessarily the binding effective now.

## HTTP contract

`GET /api/v1/tenants/{tenant_id}/signal-bindings/{binding_id}/revisions/{revision}`
with `Authorization: Bearer <token>` returns:

```json
{
  "id": "binding-1",
  "tenant_id": "tenant-a",
  "revision": "1",
  "recorded_at_ms": 2000,
  "source": {
    "device_asset_id": "gateway-1",
    "endpoint_id": "can-primary",
    "signal_id": "bus.voltage"
  },
  "target": {
    "asset_id": "power-system-1",
    "property": { "id": "electrical.voltage", "version": 1 }
  },
  "effective_interval": { "start_ms": 1000, "end_ms": 3000 }
}
```

`source` identifies what a decoder reported at a device endpoint. `target`
identifies the asset and exact semantic property receiving that signal. The
route does not decode data or evaluate whether the binding applies to an
observation. Resolve the pinned property through the
[property-definition query](property-query.md).

`revision` is decimal text in the response to preserve the complete positive
`u64` range in JavaScript clients. The path accepts only positive decimal
integer revisions in that range. Times are signed Unix milliseconds;
`recorded_at_ms` says when FleetIQ learned this revision. The physical
effective interval is `[start_ms, end_ms)`; a `null` end stays open.

Invalid identifiers or revisions return `400`; an absent exact revision
returns `404`. Missing or rejected credentials return a generic `401` bearer
challenge, a denied current grant returns `403`, and operational failures
return a generic `500` problem. Responses include `Cache-Control: no-store`.
The route is absent without complete HTTP bearer identity configuration.

The secret-injection and TLS requirements in the
[asset query procedure](asset-query.md) apply unchanged. See
[ADR 0085](../adr/0085-read-exact-signal-binding-revisions.md).
