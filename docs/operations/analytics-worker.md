# Trusted calculator worker

## TL;DR

`cargo run -p fleetiq-server --locked -- analytics-worker` is an explicit opt-in
worker for existing first-party declarations. Apply migrations separately, supply
`DATABASE_URL`, and optionally set `FLEETIQ_CALCULATOR_SCHEDULE_FILE` to install one
immutable trusted schedule. Normal HTTP startup never starts this worker.

## Schedule installation

The file is strict JSON, at most 4096 bytes. For synthetic identifiers, its shape is:

```json
{"tenant":"tenant-a","schedule":"voltage-mean","calculator":"mean","version":"1","configuration":"mean-config","scope":"asset-a","target":"result","first_start":0,"window_ms":3000,"lateness_ms":0,"minimum":1}
```

Use a deliberate first start: zero requests historical catch-up from the epoch.
All times are integer Unix milliseconds. Window size is 1..604800000, lateness
0..604800000, minimum 1..1024. The calculator and both assets must already exist;
the target must declare the calculator's output property. Exact installation retries
are accepted; changing an existing schedule ID fails. Protect the file and database
credentials as trusted operator inputs. No HTTP activation is exposed here.

## Processing and recovery

The worker processes all installed schedules for active tenants. Each turn queues
at most 32 finalized half-open windows for the oldest due schedule and claims one
work item. Known-at equals end plus lateness; first processing time is captured
once during enumeration. Multiple workers coordinate with database row locks.
Statements are limited to five seconds, turns to fifteen; each turn pauses 250 ms.

Claims last 30 seconds according to database time. Crashes, preparation errors and
ambiguous completion leave durable evidence. Expired claims can retry up to three
generations with the original request and committed preparation. Exhausted work is
retained in `analytics.calculator_work`; there is no operator reset command yet.
Preparation overflow therefore exhausts rather than silently dropping inputs.
Terminal output and done state commit together under the current unexpired fence.
Suspended tenants cannot seed, claim or complete work.

## Rollout and rollback

Apply migration 0041 before starting the checked-schema worker. Start without a
schedule file to resume existing schedules. SIGINT/SIGTERM cancels the current turn;
its claim can recover after expiry. Stop the worker to roll back execution, keeping
schema, schedules, preparations and terminal evidence. This slice adds no Compose
service, production rollout or dashboard. See [ADR 0126](../adr/0126-schedule-fenced-calculator-work.md).
