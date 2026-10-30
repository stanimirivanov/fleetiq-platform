# Alert acknowledgement

## TL;DR

Baseline.26 records an operator acknowledgement of one exact open alert version.
It does not resolve, silence, assign or suppress delivery. See
[ADR 0133](../adr/0133-acknowledge-exact-alert-versions.md).

## Authority and command

Delegate exact tenant alerts.acknowledge through existing audited tenant access
administration. Existing grants are unchanged and no grant implies this action.
Acknowledgement does not require or grant alerts.read or rule management. The
closed action vocabulary now has nine actions: regenerate clients before rollout
and delegation, including strict response validators for optional read annotations.

PUT `/api/v1/tenants/{tenant_id}/alerts/{rule_id}/revisions/{revision}/acknowledgement`
with `{"expected_version":"2"}`. The version is the observed lifecycle phase version
from alert reads, separate from the immutable rule revision in the path. Both are
canonical positive decimal; rule revision is u32 and phase version is signed-64-bit
text. Bodies are closed and limited to 1024 bytes; query parameters are rejected.
Actor/time are supplied by the host/database, never accepted from the caller.

Use the listener's configured workload bearer or browser session. Browser commands
require one exact configured Origin plus X-FleetIQ-CSRF: 1. Authentication, fresh
grants and active actor/tenant state are checked on every request; database writes
recheck/lock authority even for a retry using previously captured host evidence.

## Retry, concurrency and interpretation

The first command requires the current pending/firing version. It returns
`{version,actor_id,recorded_at_ms}` with exact string version/time. Same-author
retries return the original receipt, including after later resolution, without
restoring any prior condition. Another author cannot replace that receipt.
Missing/foreign/unobserved/closed targets and stale versions return bounded 409;
invalid input is 400, rejected credentials 401 and denied authority 403.

The checkpoint lock serializes acknowledgement and calculator evidence transitions.
If resolution wins first, an older expected version conflicts. If acknowledgement
wins, both its original receipt and the later resolution remain. Acknowledgement
changes no phase, timer, version, selection, evidence or notification intent.
A new phase version requires a separate acknowledgement; a pending receipt does
not automatically acknowledge a later firing version. All responses are no-store.

Existing [alert reads](alert-reads.md) include optional nullable acknowledgement
on each transition, including latest_transition in queue/detail. The annotation is
for that exact phase version; historical receipts remain visible after current
state moves on. Original transition facts are immutable, but a later operator
annotation can appear after a page was read. Evidence still requires its separate
telemetry.read grant. No GET on the command resource, reset, bulk or overwrite
operation is added.

## Rollout and rollback

Apply 0048 and deploy compatible checked-schema binaries together. The migration
briefly locks grant/audit tables while expanding allowed actions and creates empty
append-only acknowledgement storage. No grants or existing state are rewritten.
Keep audit/history on rollback; stop affected traffic and deploy a compatible
correction instead of deleting receipts or downgrading the migration ledger.
