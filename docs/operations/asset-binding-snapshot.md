# Protected asset-binding snapshot

## TL;DR

Read a bounded page of signal bindings targeting one asset at explicit
physical and recorded times. A configured bearer credential and a current
tenant `metadata.read` grant are required.

## HTTP contract

`GET /api/v1/tenants/{tenant_id}/assets/{asset_id}/signal-bindings?effective_at_ms=1500&known_at_ms=2500&limit=50`
with `Authorization: Bearer <token>` returns:

```json
{
  "asset_id": "power-system-1",
  "effective_at_ms": 1500,
  "known_at_ms": 2500,
  "bindings": [
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
  ],
  "next_after": null
}
```

Both times are required signed Unix milliseconds. `known_at_ms` includes
revisions recorded at that instant. Each selected latest-known revision must
still target the requested asset and be effective at `effective_at_ms`; a
correction can move it to another asset. Effective intervals are half-open.
`revision` is decimal text to preserve the full `u64` range for JavaScript.

`limit` defaults to 50 and accepts 1 through 100. When `next_after` is not
`null`, repeat the same asset, times, and limit with `after=<next_after>`.
The cursor is an exclusive binding ID. A missing asset returns `404`; an
existing asset without matching bindings returns `200` with an empty array.
Invalid input returns `400`. Authentication, authorization, generic errors,
`Cache-Control: no-store`, secret injection, and TLS behavior follow the
[protected asset query](asset-query.md).

The recorded cutoff does not freeze commits across requests. Restart the
traversal if a backdated correction arrives while paging and a consistent
view matters. See [ADR 0089](../adr/0089-page-bitemporal-asset-target-bindings.md).
