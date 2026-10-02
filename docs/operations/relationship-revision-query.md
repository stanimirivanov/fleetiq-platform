# Protected relationship revision query

## TL;DR

Read one exact recorded relationship revision with a configured bearer
credential and a current tenant `metadata.read` grant. The response is a
historical fact, not necessarily the current graph state.

## HTTP contract

`GET /api/v1/tenants/{tenant_id}/relationships/{relationship_id}/revisions/{revision}`
with `Authorization: Bearer <token>` returns:

```json
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
```

`revision` is a decimal string in the response to preserve the full positive
`u64` range in JavaScript clients. The path accepts only positive decimal
integer revisions in that range. Times are signed Unix milliseconds;
`recorded_at_ms` says when FleetIQ learned this revision, while
`effective_interval` says when it applies in the physical world. The interval
is `[start_ms, end_ms)`; a `null` end remains open. The source and target are
ordered, directed roles. Resolve the pinned type's name through the
[relationship-type query](relationship-type-query.md).

Invalid identifiers or revisions return `400`; an absent exact revision returns
`404`. Missing or rejected credentials return a generic `401` bearer
challenge, a denied current grant returns `403`, and operational failures
return a generic `500` problem. Responses include `Cache-Control: no-store`.
The route is absent without complete HTTP bearer identity configuration.

The secret-injection and TLS requirements in the
[asset query procedure](asset-query.md) apply unchanged. See
[ADR 0084](../adr/0084-read-exact-relationship-revisions.md).
