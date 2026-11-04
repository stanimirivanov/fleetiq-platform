# ADR 0134: Deliver durable incident notifications to Alertmanager

## Status

Proposed, 31 October 2026.

Supersedes: none. Superseded by: none.

## Context

ADR 0131 retains live firing/resolved notification intents atomically with exact
alert transitions. It performs no remote I/O. M05 needs an explicit trusted worker
that survives restart, retries uncertain remote effects and refreshes firing state
without claiming receiver delivery or silently resolving missing evidence.

## Decision

Add an opt-in notification-worker mode requiring DATABASE_URL and one operator-set
FLEETIQ_ALERTMANAGER_URL, with read-only checked-schema startup. HTTP serving and
analytics-worker never start it. A destination digest plus tenant/rule/revision and
original firing version identifies a durable delivery incident. The same labels
and original event-time startsAt survive retries, refresh and resolution; a later
firing episode has a different remote identity, insulating it from old retries.

Preserve all original intents. Explicit seeding adds at most 32 firing incidents
per turn for that destination. Claims select the first terminal successor afresh:
a retained resolution supersedes an unsent firing snapshot. This is current-incident
at-least-once delivery, not a promise that every historical transition produces a
separate receiver notification. New endpoints have independent acceptance state.

Use database-clock thirty-second leases, monotonic generation fences and five
attempts per target/refresh cycle, including expired claims. Failure backs off
2/4/8/16/30 seconds. Success resets the cycle and schedules a thirty-second refresh.
A newer resolution rearms an exhausted firing cycle. Retain exhaustion for later
operational recovery; do not introduce an unaudited reset command here. Active
snapshots expire remotely after two minutes; accepted resolutions reconcile for
three minutes without extending that horizon. Database fences cannot recall an
already accepted remote request: finite validity and resolved reconciliation bound
stale effects. Host/database/router clocks must agree; capacity/skew qualification
remains outside this slice.

Only active tenants may claim or complete work. Rule deactivation does not invent
resolution: already-firing incidents keep refreshing. Inactive tenants pause
outbound work; remote expiry does not alter FleetIQ lifecycle truth. No
acknowledgement or metadata permission changes delivery policy.

The server adapter owns the already-locked reqwest 0.12.28 and time 0.3.55 directly.
Use only Rustls TLS and UTC RFC3339 formatting, respectively. Both offer MIT or
Apache-2.0 licensing, support the pinned compiler, and add no resolver versions.
PostgreSQL owns durable fencing; analytics owns native snapshots. No HTTP/date
library enters policy or domain crates. The adapter sends one alert, at most 16 KiB,
to the exact configured /api/v2/alerts path, with five-second total/two-second
connect timeouts. Reject URL credentials, query/fragment, redirects and environment
proxies. Trust ordinary TLS verification. Read at most 8 KiB of a successful
response and retain only closed failure categories. HTTP 200 acceptance is not
proof of receiver routing, delivery or human acknowledgement.

## Consequences

Migration 0049 creates only new empty state and an index; it does not send or
rewrite intents, backfill lifecycle facts or change grants. Runtime roles need
SELECT on source identities and SELECT/INSERT/UPDATE on delivery state. Deploy
checked-schema binaries after migration; stop the worker to roll back side effects,
retaining history and state. A changed endpoint independently replays retained
incident snapshots; stop old workers before intended endpoint replacement.

The bounded single-incident loop is not a throughput/SLO promise. Large retained
history and router outage/exhaustion need operational qualification. Packaging and
private Alertmanager routing remain PR16; diagnostics/recovery and installed
receiver qualification remain PR17–20. No tenant-chosen destinations, credentials,
public Alertmanager administration or notification suppression is added.

## Alternatives considered

A second broker/workflow engine would duplicate existing durable work patterns.
Per-intent blind replay could reopen resolved incidents. Sending active alerts
once would let them expire after a restart or temporary router loss. Exactly-once
remote delivery cannot be established by a database transaction. A custom calendar
or TLS/HTTP implementation would replace already-locked maintained libraries with
unnecessary risk. Initial protocol follows the upstream
[Alertmanager v2 schema](https://github.com/prometheus/alertmanager/blob/main/api/v2/openapi.yaml).
