# Audited work recovery

## TL;DR

Use exact operational authority, a stable request ID and the observed generation.
Retry reuses original work and advances its fence. It does not edit inputs, force
completion, resume scheduling, prove health or guarantee receiver delivery. Read
[ADR 0136](../adr/0136-recover-work-with-monotonic-fences.md) before rollout.

## Prerequisites and rollout

Stop all old server/worker binaries before migration 0051. Replay/calculator
attempt budgets are now separate from generations; mixed old/new workers are
unsupported. The migration backfills attempts from existing generations without
changing original work, preparation, source facts or grants. It flushes deferred
provenance/FK checks before re-enabling guards and adding constraints/indexes.
Backfills and ordinary index builds scan retained tables and hold locks: qualify
maintenance timing and take the installation's reviewed backup first. Action
checks change last so their locks do not span the queue backfills.

Deploy compatible checked-schema processes, then explicitly delegate
operations.manage using existing tenant access administration. Baseline.28 has
eleven actions; regenerate strict clients before delegation. No old grant implies
this capability. Runtime roles need source SELECT, work/control UPDATE, control
INSERT and audit INSERT/SELECT for the invoker-rights command; use normal restricted
runtime credentials, never DDL/superuser fallback. No DELETE is needed. Keep the
host's database credentials private; actors operate through authenticated HTTP.

## Prepare and issue one command

GET /api/v1/tenants/{tenant_id}/work-recovery/{kind} requires both operations.read
and operations.manage. Kind is replay, calculator, alert_evaluation or notification.
It returns control generation/paused state, at most 32 retryable targets with work
generations and a truncated flag. Each target is an exact key, not a payload or URL;
notification keys include an opaque destination digest. More failures can appear
when earlier targets are recovered. Unavailable execution/configuration, live
leases, completed work and finite fence capacity exclude targets. Alert evaluation
has controls only, with no invented durable failed lease. Timeouts fail closed.

PUT /api/v1/tenants/{tenant_id}/work-recovery/{kind}/commands/{request_id} requires
operations.manage. Browser writes also require the configured exact Origin and
X-FleetIQ-CSRF: 1. The closed JSON body contains operation, expected_generation
(as exact decimal text) and target. Bodies are capped at 2 KiB; unknown fields,
queries, noncanonical counters and wrong key shapes are rejected.

- retry uses the exact target/generation from preparation. Replay/calculator keys
  contain id; notification keys contain destination, rule_id, revision and
  firing_version. No arbitrary destination, payload, prepared input or SQL is accepted.
- pause/resume use an empty target and the observed **control** generation. They
  affect the tenant's whole named work family, not one rule or schedule. Missing
  control means unpaused/generation zero. A command already in that state conflicts.

Keep the same request ID and exact body after an ambiguous response. The receipt
retains original actor/time and produced generation, even if later work or controls
have advanced. It is not current state. A reused ID with changed author/body or a
stale/ineligible new command returns 409. Refresh authorized preparation before a
new decision; do not substitute a new ID merely to repeat an uncertain command.
Revocation/inactive actor/tenant is rechecked even for the original receipt.

## Effects and limits

Replay retains original pre-frame bytes, sequence and supported execution binding.
A recovered claim must still be the next session turn. Calculator retains original
request, logical time, selected inputs and evaluation ID. Three claims per explicit
cycle remain the limit, independent of the strictly increasing fence. Old/expired
workers cannot complete the replacement; finished results cannot be reopened.

Notification retry preserves incident labels, startsAt, target version, accepted
history and fixed resolved-healing horizon. It rearms only exhausted eligible work,
not a currently leased position or a completed reconciliation window. Five attempts
remain per cycle. Newer terminal evidence can rearm naturally without this command.
A late remote request cannot be retracted; stable identity/reconciliation remain
the producer's at-least-once mechanism. Receiver receipt is still not proven.

Pause stops new scheduling/claims that observe the committed control. Already
running statements and valid completion may finish. Replay ingress remains durable
and protocol replies may stall; calculator enumeration/cursors wait; alert pause
retains firing state; notification pause can let remote validity expire without
resolving FleetIQ truth. Pause is not drain, acknowledgement or silence. Retry while
paused stays paused. Use separate process/work observations before maintenance.
Diagnostics count paused eligible work as blocked while preserving lease/failure
observations; empty diagnostics are never liveness evidence.

Target reads use two-second statements; commands use three seconds, with five/six
second HTTP storage turns. No raw exception, payload or endpoint enters responses.
Audit rows and controls are retained. Roll back grants/routes with compatible
binaries; never delete evidence, reset fences or restart pre-0051 workers. UI is
PR19 and installed qualification is PR20.
