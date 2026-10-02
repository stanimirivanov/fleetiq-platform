# Protected signal-binding snapshot

## TL;DR

Read a bounded page of bindings that attributed one decoded device signal at
explicit physical and recorded times. A configured bearer credential and a
current tenant `metadata.read` grant are required.

## HTTP contract

`GET /api/v1/tenants/{tenant_id}/signal-bindings?device_asset_id=gateway-1&endpoint_id=can-primary&signal_id=bus.voltage&effective_at_ms=1500&known_at_ms=2500&limit=50`
with `Authorization: Bearer <token>` returns:

```json
{
  "source": {
    "device_asset_id": "gateway-1",
    "endpoint_id": "can-primary",
    "signal_id": "bus.voltage"
  },
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
revisions recorded at that instant. The selected latest revision must still
name the requested source and be effective at `effective_at_ms`; a correction
can therefore remove a binding from a later snapshot. The physical interval
is half-open. `revision` is a decimal string to preserve the full `u64`
range in JavaScript clients.

`limit` defaults to 50 and accepts 1 through 100. When `next_after` is not
`null`, repeat the same source tuple, times, and limit with
`after=<next_after>`. The cursor is an exclusive stable binding ID. A valid
source with no matching bindings returns `200` with an empty array; signal IDs
have no separate registration record. Invalid input returns `400`.
Authentication, authorization, errors, `Cache-Control: no-store`, secret
injection, and TLS behavior follow the
[protected asset query](asset-query.md).

The recorded cutoff does not freeze commits across requests. Restart the
traversal if a backdated correction arrives while paging and a consistent
view matters. See [ADR 0087](../adr/0087-page-bitemporal-source-bindings.md).
