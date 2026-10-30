# ADR 0131: Evaluate evidence-driven durable alert lifecycles

## Status

Proposed, 30 October 2026.

Supersedes: none. Superseded by: none.

## Context

[ADR 0130](0130-configure-versioned-alert-rules.md) pins exact numeric rules but
has no execution state. M05 needs restart-safe pending/firing/resolved transitions
and outbound intentions without sending notifications during historical replay.

## Decision

Analytics owns a pure reducer over inactive, pending, firing and resolved phases.
Use calculator window ends as event time. The first true observation starts the
hold candidate; later strictly increasing contiguous/overlapping evidence can
satisfy it. Zero duration fires immediately. Explicit false evidence similarly
starts/satisfies the resolution delay. Missing/failed/empty evidence cancels a
pending hold and clears a false candidate without resolving a firing alert.
A window gap or activation generation change breaks duration continuity.

Persist one checkpoint per tenant/rule/revision. Scope belongs to the immutable
referenced rule revision, so the identity includes its exact scope without a
second mutable copy. Each phase change increments the checkpoint version and
appends an immutable transition linked to the exact evaluation receipt. Duration
candidate changes alone are checkpoint updates, not phase transitions.

Each explicit analytics-worker turn independently processes one calculator item
and at most one rule/evaluation. Serialize on the active rule head and current
tenant lifecycle, using READ COMMITTED and a fresh statement after acquiring the
head lock. Activation/deactivation shares this lock. Choose the least recently
processed eligible rule, then visible evaluations by window end and C-collated
ID. Persist per-evaluation receipts rather than advancing a sequence cursor;
an earlier producer transaction can commit late without being permanently lost.
Equal/older event ends receive out_of_order receipts and cannot rewrite state.

Only evaluations atomically completed through durable scheduled work, for the
selected target, are eligible for live lifecycle effects. One-shot evidence is
historical even if its timestamp is recent. Scheduled window ends or evaluation
times preceding the current activation receipt or the immutable deployment/revision-creation
live boundary are historical as well. Historical
receipts do not change phases/timers and never create outbound intentions.
Reactivation resumes retained firing state but restarts duration candidates;
evidence from the inactive interval cannot retroactively create transitions.

In one transaction, retain the receipt, checkpoint, optional phase transition,
and firing/resolved notification intent. Immutable transition identity is the
outbox deduplication key. Pending/cancellation changes cannot create intents.
Cancellation and all write failures roll back the entire turn. No delivery
process, destination, retry lease or external side effect exists until PR15.

## Consequences

Migration 0046 seeds inactive checkpoints for existing revisions, adds empty
history/outbox tables and initializes future revision checkpoints automatically.
It adds an evaluation selector index; transactional construction can block writes
on a populated evaluation table, so measure and schedule an idle deployment
window. Existing evaluation rows, grants and activation selections are unchanged.

No dependencies or public HTTP contract changes. Workers require the full schema
before running. Stop workers for rollback and retain immutable history/schema.
Unknown evidence is not a healthy condition; worker downtime itself cannot satisfy
a hold or resolve delay. Event-time gaps require a fresh candidate.

Receipt anti-joins may revisit a long consumed prefix. The selector index,
one-row turn, statement deadlines and fair rule ordering bound individual work,
but retained-history scale still requires measurement. Do not replace receipts
with a max-sequence cursor without a commit-order guarantee. Equal-end conflicting
outputs across configurations use the first consumed result; later equal ends are
retained without changing lifecycle meaning.

## Alternatives considered

Adapt Prometheus's pending/firing condition pattern, retaining FleetIQ's explicit
event-time and unknown-evidence semantics. Use the transactional outbox pattern
with existing PostgreSQL/SQLx; no expression engine or new library is necessary.
A max-ID cursor can skip a late commit. Worker-local timers lose duration history
on restart. Sending inline cannot atomically preserve state and external delivery.
Treating one-shot replay as live can silently notify about historical conditions.
