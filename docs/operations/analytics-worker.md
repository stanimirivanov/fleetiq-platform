# Calculator configuration and worker

## TL;DR

Apply migrations separately, supply `DATABASE_URL`, then run
`cargo run -p fleetiq-server --locked -- analytics-worker`. Only explicitly activated
configuration revisions run. Normal HTTP startup never starts this worker.
Migration 0043 imports old trusted schedules as **inactive** revision 1 and rejects
the transitional `FLEETIQ_CALCULATOR_SCHEDULE_FILE` option.

## Configuration API

Under `/api/v1/tenants/{tenant_id}/calculator-configurations/{configuration_id}`:

- `PUT /revisions/{revision}` creates the next revision, starting at 1. The JSON
  fields are `calculator`, `version`, `scope`, `target`, `first_start`, `window_ms`,
  `lateness_ms` and `minimum_samples`. No actor or receipt time is accepted.
- `GET /revisions/{revision}` returns exact parameters and original attribution.
- `GET /activation` returns generation, selected revision, author and receipt time.
- `PUT /activation` accepts `{"expected_generation":0,"revision":1}` to select a
  revision; explicit `"revision":null` deactivates. Omission is invalid.

Reads require `metadata.read`, writes require separate `analytics.manage`. Delegate
through existing audited tenant grant administration; other grants do not imply
this capability. Browser writes require the configured Origin and CSRF header.
Baseline.22 expands the closed action vocabulary; regenerate clients before
advertising the sixth action. JSON is strict and bounded to 4096 bytes.

A revision pins an admitted calculator and tenant-owned scope/output target with
registered property membership. Window size is 1..604800000 milliseconds, lateness
0..604800000, minimum 1..1024. Times are signed Unix milliseconds. Choose a deliberate
first_start: zero requests epoch catch-up. The mean/MAD implementation keeps its
own versioned minimum and unit semantics.

Stale revisions/generations or unavailable compatible references return 409.
Same-author/content retries return original receipts and never restore old active
state. New revisions are inactive while an already selected revision keeps running.

## Processing and recovery

Each turn queues at most 32 finalized half-open windows for the oldest due active
revision and claims one item. Known-at equals end plus lateness; first processing
time and preparation are retained. Statements have five-second deadlines, turns
fifteen seconds and pauses 250 ms. Database-clock leases last 30 seconds with at
most three generations; exhaustion is retained without an operator reset API.

Activation shares the configuration lock with enumeration/claims. Concurrent
claims skip busy configuration heads and may return idle while windows remain;
the next worker turn retries after contention. Deactivation
pauses unclaimed work; issued claims may complete atomically with their original
evidence. Switching revisions retains old pending work without running it. Selecting
the old revision again resumes its original cursor and saved requests.

## Alert lifecycle

Each turn also independently consumes one selected alert-rule evidence item.
Checkpoint, evidence receipt, optional transition and notification intent commit
atomically; failure in calculator execution does not block this consumer. Only
completed scheduled work at/after the rule's activation boundary can affect live
state; one-shot/historical evidence cannot silently notify. No network delivery
runs. See [alert lifecycle operation](alert-lifecycle.md) for timing, missing-data,
replay, deactivation and migration rules.

## Rollout and rollback

Stop old workers, apply 0043 and deploy updated checked-schema binaries. Read imported
legacy schedules using their schedule ID as configuration ID and revision 1; null
author means unknown trusted installation attribution. Explicitly grant and activate.
SIGINT/SIGTERM cancels a turn; its lease recovers after expiry. Stop the worker to
roll back execution, retaining schema, revisions, preparations and output evidence.
No production service or editor UI is packaged here. See [ADR 0128](../adr/0128-activate-calculator-configurations.md).

Use [audited recovery](work-recovery.md) for failed work or tenant queue controls.
After 0051, attempts are per cycle and generations are monotonic across recovery;
old binaries must not run. Pause is not a drain or an input reinterpretation.
