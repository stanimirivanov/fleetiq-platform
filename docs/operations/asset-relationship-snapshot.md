# Protected asset relationship snapshot

## TL;DR

Read a bounded page of edges involving an asset at explicit physical and
recorded times. A configured bearer credential and current tenant
`metadata.read` grant are required.

## HTTP contract

`GET /api/v1/tenants/{tenant_id}/assets/{asset_id}/relationships?effective_at_ms=1500&known_at_ms=2500&limit=50`
with `Authorization: Bearer <token>` returns:

```json
{
  "asset_id": "assembly-a",
  "effective_at_ms": 1500,
  "known_at_ms": 2500,
  "relationships": [
    {
      "id": "relationship-1",
      "tenant_id": "tenant-a",
      "revision": "1",
      "recorded_at_ms": 2000,
      "relationship_type": { "id": "physical.contains", "version": 1 },
      "source_asset_id": "assembly-a",
      "target_asset_id": "component-b",
      "effective_interval": { "start_ms": 1000, "end_ms": 3000 }
    }
  ],
  "next_after": null
}
```

Both times are required signed Unix milliseconds. `known_at_ms` includes
facts recorded at that instant; the physical effective interval is half-open.
The latest revision known at the cutoff must still involve the asset and be
effective at `effective_at_ms`. A correction can therefore remove an edge
from a later snapshot. The response includes both source and target roles.

`limit` defaults to 50 and accepts 1 through 100. When `next_after` is not
`null`, request the next page with `after=<next_after>` and the same times and
limit. The cursor is an exclusive stable relationship ID. A missing asset
returns `404`; an existing asset without matching edges returns `200` with an
empty array. Asset existence comes from the current catalogue; only the
relationships are bitemporal. Invalid input returns `400`. Other authorization,
error, `Cache-Control: no-store`, secret-injection and TLS behavior follows the
[protected asset query](asset-query.md).

The recorded cutoff does not freeze database commits across requests. A
backdated correction committed during traversal can alter later pages; restart
the traversal when a consistent view matters. See [ADR 0086](../adr/0086-page-bitemporal-asset-relationships.md).
