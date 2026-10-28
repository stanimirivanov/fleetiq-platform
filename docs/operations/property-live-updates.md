# Exact-property live updates

## TL;DR

Use the same-origin SSE `/live` sibling of the exact-property `/current` read.
Supply only a last successfully applied opaque SSE id to resume. This stream
replaces latest state; use history for intermediate interpretations. The
configured identity needs telemetry.read. D06 and hardware qualification remain
deferred.

## Open and apply

Run the existing `project-current` worker as described in
[current-state operations](current-property-state.md). The HTTP stream only
observes committed projection state; it does not start a projector or claim
worker/device health.

1. Select an exact tenant, asset, declared property and immutable version.
2. Open a same-origin GET with `Accept: text/event-stream`. Browser sessions use
   their existing secure cookie; workload credentials remain server-side.
   Credentials never belong in the URL.
3. With no Last-Event-ID, apply the `snapshot` data atomically before retaining
   its id. Data uses the existing CurrentPropertySnapshot schema and exact strings.
4. Apply `replace` only if the held generation matches the snapshot generation
   and held revision equals base_revision. Replace all property contents, then
   retain the new id. A revision jump is legal coalescing, not missing history.
   Tenant revisions may advance for another property, so identical replacements
   are not new observations; immutable interpretation identity owns evidence.
5. A `heartbeat` samples pending/evaluation time without an id. Recalculate age
   from the held event time and stale_after_ms; never upgrade invalid quality or
   infer connectivity/health. Heartbeats never advance the cursor.
6. Parse complete SSE events through their blank-line delimiter; do not assume
   id/data field order or HTTP chunk boundaries.

For a configured authenticated operator, a private cookie-jar diagnostic is:

```sh
curl --no-buffer --cookie /private/session-cookie-jar   --header 'Accept: text/event-stream'   'https://fleet.example/api/v1/tenants/tenant-a/assets/asset-a/properties/electrical.voltage/versions/1/live'
```

Protect that cookie jar outside Git. The browser normally manages it; this guide
does not prescribe extracting or logging browser credentials.

## Resume and recovery

Reconnect to the identical scope with `Last-Event-ID: <last-applied-id>`.
The id is scope-bound, not authorization. The server rechecks current credentials,
original actor, mapping and tenant grant around each consistent read. Equal
positions start with a heartbeat; later positions get a complete replacement.
More than 256 forward tenant revisions, another generation or a future cursor
produce terminal `resnapshot-required`. A delay over 30 seconds between demands
uses its slow_consumer reason. Clear the baseline/id and reconnect without the
header. Do not retry the same rejected id forever.

`stream-ended` reasons require distinct handling:

| Reason | Operator/client action |
| --- | --- |
| authentication_required | Close, clear protected state and sign in again. |
| access_denied / scope_missing | Close, clear state and resolve authority/scope. |
| unavailable | Close; use bounded backoff without inventing empty values. |
| lease_expired | Close; reconnect with the last applied id. |
| payload_limit | Close; investigate the property payload before retrying. |

Every terminal event lacks an id and closes the stream. Explicitly close native
EventSource on these events to suppress its automatic retries. For transient
network/unavailable failures use jittered bounded backoff and a finite retry
budget, then show manual recovery. Account/tenant/asset/property changes must
cancel the old stream and drop its baseline/id. PR 27 implements this UI policy.

## Bounds and deployment

- 64 open subscriptions per server process; capacity rejection is HTTP 429 with
  Retry-After 2. There is no queued connection wait.
- Demand-driven reads at most once every two seconds; idle heartbeats are around
  16 seconds because checks occur on that cadence.
- 60-second lease and 30-second demand-gap checks run at body demand. They are
  not TCP socket deadlines for an entirely unpolled body.
- Eight-second authority/read deadline; existing snapshot SQL deadlines remain.
- JSON serialization stops at 60 KiB; SSE frames remain below 64 KiB.
- Cache-Control no-store and X-Accel-Buffering no. Preserve streaming, disable
  intermediary buffering and configure proxy write/idle timeouts. Do not cache,
  compress into long buffered batches or rewrite SSE frames.

Queued bytes already in HTTP/proxy/kernel buffers cannot be recalled after a
revocation. Future reads and frames are freshly checked, including the first
body demand. Disconnect/drop cancels pending read futures and releases capacity;
no producer continues polling after disposal. Multiple processes multiply the
local limit; installation-wide admission/rate budgets require deployment policy.
No live-performance or large-fleet capacity claim follows from functional tests.

Normal process restart resumes from durable positions. Restoring an older or
divergent database lineage is separate recovery work: invalidate old cursors
before admitting clients. This feature does not qualify disaster recovery,
D06 or full-profile deployment.

## Verification

Pure policy tests cover initial/duplicate/coalesced/future/reset/expired positions.
Controlled-clock stream tests cover real SSE framing, heartbeat/lease, bounded
serialization, slow demand, authority failures and cancellation. PostgreSQL HTTP
and actual TCP tests cover admission/disposal, initial-body reauthorization,
committed reconnect values and revoked grants; browser tests revoke a session
mid-stream. Run heavy database/Rust/transport checks on CI. A contract bundle
alone is not evidence of a working deployment.
