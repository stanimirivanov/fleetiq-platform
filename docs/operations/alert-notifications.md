# Alert notification delivery

## TL;DR

Run the explicit notification-worker only after migration 0049 with one trusted
operator-configured Alertmanager v2 endpoint. HTTP/analytics processes never send
notifications. Queue acceptance means HTTP acceptance, not receiver delivery.
See [ADR 0134](../adr/0134-deliver-durable-alert-notifications.md).

## Start, configuration and stop

Set DATABASE_URL using the deployment's private mechanism and
FLEETIQ_ALERTMANAGER_URL to the complete http(s) endpoint ending /api/v2/alerts.
Then run `cargo run -p fleetiq-server --locked -- notification-worker`.
The endpoint is not tenant-controlled. URL credentials, query, fragment and other
paths are rejected; no environment proxy or redirect is followed. HTTPS uses normal
Rustls certificate verification. Keep private HTTP transport inside the trusted
network. Receiver secrets/routing and workload packaging belong to PR16; this
process does not expose HTTP health or an Alertmanager administration listener.

Startup checks the entire embedded migration ledger without DDL. Four database
connections retain five-second statement/three-second acquisition limits. Each
turn seeds at most 32 incidents and delivers one snapshot, with a fifteen-second
turn timeout, five-second HTTP timeout and 250 ms pacing. SIGINT/SIGTERM cancels
in-flight work and closes the pool. A cancelled effect remains uncertain and its
lease eventually expires; no success is invented.

## Identity and retained meaning

Labels include tenant, exact rule revision, scope, target and original firing
version. Stable labels and startsAt survive restart, retries and resolution. A new
firing episode gets a separate identity, so delayed older work cannot resolve it.
Annotations link the exact terminal transition and calculator release, not raw
telemetry, prepared values or customer payloads. Serialized requests are capped at
16 KiB. Only status 200 with a response no larger than 8 KiB confirms acceptance.

The worker preserves immutable intents and transitions. A resolution retained
before claim supersedes an obsolete firing snapshot. Delivery may therefore
coalesce fast firing/resolution into a resolved snapshot, rather than send a fresh
historical firing alarm. A configured destination digest owns its own progress;
changing the endpoint independently replays retained incident snapshots. Stop old
workers before an intended endpoint replacement. Alertmanager routing/receiver
access must keep tenant labels correctly separated; labels alone are not an
access-control boundary.

## Refresh, failure and recovery

Database-clock leases last thirty seconds. Completion requires the current,
unexpired generation/target and active tenant; late responses cannot confirm a
replacement claim. Each cycle allows five attempts, including crashes with expired
leases, with 2/4/8/16/30-second failure backoff. Exhaustion remains retained; there is
no unaudited reset/replay command. A newer resolution rearms an exhausted firing
cycle. Later PRs add operational diagnostics and audited recovery.

Firing snapshots use a two-minute endsAt and refresh every thirty seconds after
success. Resolved snapshots reconcile for three minutes after their first accepted
response. That fixed window exceeds previously advertised active validity and
heals late/ambiguous effects. Database fencing cannot retract network requests;
this is at-least-once current-incident convergence, not exactly-once notification.
Synchronize database and router clocks, and qualify scale/skew before rollout.
A large backlog can exceed the refresh cadence; no capacity or SLO is implied.

Rule deactivation pauses evaluation without resolving a retained firing condition,
so delivery keeps refreshing it. Tenant deactivation pauses claim/completion; a
remote expiry does not change FleetIQ phase. Acknowledgement never silences,
resolves or suppresses delivery. Logs/last_failure contain bounded categories
(transport, rejected, response_limit, invalid_payload), never URLs, bodies or
credentials. Failed refresh does not prove health or receiver receipt.

## Migration and rollback

0049 creates empty destination/incident work state and its due index. It scans no
historical data during migration; explicit worker seeding consumes retained intents.
It briefly locks only new objects and source references during DDL. Grant the
runtime role SELECT on source tenants/rule revisions/transitions/intents and
SELECT/INSERT/UPDATE on analytics.alert_notification_delivery. Do not grant DDL,
ledger mutation or a superuser fallback. No DELETE is needed.

Migrate first, then deploy compatible checked-schema workers. Stop this process to
stop new outbound attempts, preserving immutable history and delivery state. An
older binary cannot silently accept the new ledger; roll back through a compatible
forward build. Image/Compose/Kubernetes packaging and real receiver qualification
are separate tasks; loopback fixtures do not establish installed delivery.
