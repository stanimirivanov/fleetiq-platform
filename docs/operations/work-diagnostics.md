# Operational work diagnostics

## TL;DR

GET /api/v1/tenants/{tenant_id}/work-diagnostics requires exact operations.read.
The response describes bounded samples, never total backlog, worker health or
receiver delivery. Delegate this tenth action explicitly after migrating through
0050 and updating strict baseline.27 clients. No existing actor gains access.

## Meaning and bounds

One read-only repeatable database view samples at most 64 identities per family,
plus a sentinel for truncated coverage. Four observations always appear, including
empty families. Counts describe only those observations. The oldest sampled
eligible timestamp is a receipt, finalized-window, next-evaluation processing or
notification-due time, depending on family. It is not a global oldest or a health
threshold. Milliseconds are exact decimal strings, including nullable oldest time.

- Replay excludes completed positions; reports active lease, poison/expired final
  attempt, incompatible execution policy or predecessor blocking and eligibility.
- Calculator excludes done positions; reports active lease, retained/expired final
  attempt, inactive configuration selection and eligibility.
- Alert evaluation samples selected rule heads, then uses the same next-evidence
  selector as the worker. A rule contributes at most one eligible input. There is
  no durable per-rule lease/failure record; idle does not mean the worker is healthy.
- Notification samples retained destination/incident positions; reports leases,
  deferred refresh/backoff, exhaustion and eligibility. A newer terminal successor
  rearms an exhausted target just as in the worker. Completed reconciliation
  positions may occupy the prefix. Endpoint digests and configuration are omitted.

max_attempts is the highest attempt count in the sample, not a sum. Failure
categories are closed: attempts_exhausted, transport, rejected, response_limit and
invalid_payload. Earlier replay/calculator stores do not retain exception reasons;
no reason is invented. Early poison may have an empty failure-category list; it
is not labelled attempts_exhausted until its attempt budget is spent. notification_seeding and worker_liveness are always
not_observed: absence of seeded positions cannot prove no notification demand.
Truncation concerns positions/rules, not all future evidence behind a selected rule.

The statement deadline is two seconds and the HTTP storage turn is five seconds.
Tenant-first prefix indexes bound sample discovery; the existing alert selector
still depends on retained history and can time out. Timeouts produce a redacted
500 problem, not a partial success. Qualify installed volume/skew before rollout.
No query parameter, raw payload, telemetry value, URL, key or log expansion is
provided. Reads never claim, retry, release, acknowledge or modify work.

## Authority, compatibility and operation

operations.read is independent of metadata/telemetry/protocol/analytics/alerts
access and implies no recovery or management. Existing grant administration and
operator discovery expose it; tenant/actor activity and current grants are checked
before discovery. Existing asset ingestion diagnostics retain protocol.manage;
use that device-specific route for bounded frame/source evidence when authorized.
Both APIs use no-store and separate browser/workload authentication modes.

Runtime roles require SELECT on these queues, source schedules/selected rules,
replay sessions/accepted receipts and alert selector sources. Migration 0050 adds
indexes and extends action checks. Index builds scan retained tables and may block
writes; review maintenance timing against installed data. Stop using/revoke the
new capability to roll back reads while retaining source state. A compatible
forward binary is required for the new ledger/action vocabulary. See
[ADR 0135](../adr/0135-read-bounded-operational-work-evidence.md).

Use [audited recovery](work-recovery.md) for independent operations.manage commands.
Paused eligible positions are blocked; max_attempts is per cycle, not the fence.
PR19 owns the operational UI. These samples do not
replace installed worker/router qualification or production monitoring.
