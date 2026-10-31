# Durable alert lifecycle operation

## TL;DR

The explicit `analytics-worker` now advances selected alert-rule revisions and
retains notification intents in the same transaction. An opt-in [notification worker](alert-notifications.md) now consumes retained
incidents with bounded retry/refresh/reconciliation; analytics-worker itself never sends them.
Protected [alert reads](alert-reads.md) now expose observed state and bounded history. See [ADR 0131](../adr/0131-evaluate-durable-alert-lifecycles.md).

## Start and stop

Apply migrations through the existing migration entrypoint, then use the existing
`cargo run -p fleetiq-server --locked -- analytics-worker` with `DATABASE_URL` set.
HTTP startup does not launch this worker. Only explicitly selected rule revisions
in active tenants are eligible. Stop with SIGINT/SIGTERM; cancellation rolls back
an incomplete turn. Multiple workers serialize each rule head and skip busy heads.

Each turn independently runs calculator scheduling and one alert evidence item.
A calculator failure does not starve retained alert evidence. Statements retain
five-second deadlines, each side has a fifteen-second timeout and turns pause
250 ms. Four pool connections remain sufficient. Last-processed ordering prevents
one eligible rule from monopolizing the consumer.

## Evidence and timing

Exact release, scope, target, property/version, ordinal and unit come from the
immutable selected rule. A successful envelope for another target is not consumed.
Empty/failed matching envelopes are unknown evidence. Only an evaluation completed
atomically with durable scheduled work for this target can change lifecycle.
Trusted one-shot calculations are historical receipts, even with recent timestamps.
A scheduled event end or evaluation time older than either the current activation
receipt or the checkpoint's immutable deployment/revision-creation live boundary
is also historical. Neither kind creates transitions or notification intents.

The first true observation end starts a pending hold. Later increasing window
ends can satisfy it; a zero hold fires immediately. Explicit false observations
start/satisfy the separate resolution delay. Missing evidence cancels pending
and clears any false candidate; it never resolves firing. A gap between windows
or activation generation change restarts candidates. Deactivation pauses the
consumer without pretending that a firing condition resolved. Reselecting a
revision retains its phase/history but cannot count the inactive interval.

Visible evidence is ordered by window end and C-collated evaluation ID. Durable
receipts prevent duplicate consumption across retries/restart. Late commits are
still discovered; older/equal event ends receive out_of_order receipts without
changing timers/phases or sending fresh notifications. If configurations produce
conflicting equal-end evidence, the first consumed result owns that event point.
No wall-clock timer advances during worker downtime or missing observations.

## Persistence, rollout and recovery

`analytics.alert_lifecycles` stores mutable revision-local checkpoints.
`alert_evaluation_receipts` records applied/missing/historical/out_of_order input.
`alert_transitions` links immutable phase changes to exact evidence. Only firing
and resolved transitions create `alert_notification_intents`, atomically with the
checkpoint and receipt. Phase-transition version is the stable outbox identity;
duration-only updates do not increment it. Scope is retained through the immutable
rule revision rather than duplicated in a mutable row.

0046 initializes existing/new revision checkpoints inactive without selecting a
rule or consuming old evidence. It adds an ordered selector index on evaluations:
measure retained volume and use an idle deployment window because transactional
index construction can block evaluation appends. Deploy schema-compatible workers
together. Preserve receipts, transitions and intents on rollback; stop workers and
ship a compatible correction rather than deleting history or downgrading the ledger.
The consumer may scan a long receipted prefix; monitor turn failures/timeouts and
measure large-history behavior before increasing throughput. There is no receipt
reset, historical replay notification override or delivery command in this slice.
