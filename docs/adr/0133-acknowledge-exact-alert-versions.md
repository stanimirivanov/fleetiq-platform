# ADR 0133: Acknowledge exact alert phase versions

## Status

Proposed, 30 October 2026.

Supersedes: none. Superseded by: none.

## Context

[ADR 0132](0132-publish-bounded-alert-reads.md) exposes observed alert versions
and exact evidence. Operators need to record that they noticed a condition without
claiming that the condition resolved, silencing it or changing delivery policy.

## Decision

Add independently delegated alerts.acknowledge. Neither alerts.read nor rule
management grants imply it, and it implies neither of those actions. Expand the
closed action vocabulary and persisted grant/audit constraints together without
adding grants. Reuse actor-attributed immutable command/audit and exact-retry
patterns with existing dependencies.

PUT the exact alert path plus /acknowledgement with expected_version as canonical
positive signed-64-bit decimal text. The server supplies authenticated actor and
the database supplies receipt time. First acknowledgement requires the current
pending/firing phase version. A single immutable receipt belongs to each
tenant/rule/revision/version identity. Another author conflicts; a same-author
retry returns the original receipt even after later phase changes, provided
current authority and actor/tenant lifecycle still permit the command.

Use explicit READ COMMITTED, recheck/lock current action grant, actor and tenant,
then lock the lifecycle checkpoint shared by evidence processing. No rule-head
lock is acquired after that checkpoint. If a resolve transition wins, the older
expected version conflicts; if acknowledgement wins, its receipt survives the
later resolution. Audit failure rolls back without a partial receipt.

Acknowledgement never updates phase, phase-transition version, timers, selected
rule revision, evidence, transitions or notification intents. A later phase
version needs its own acknowledgement; a pending acknowledgement does not
silently acknowledge a later firing condition. Closed/unobserved/missing/foreign
targets share a bounded conflict outcome. Commands need no read grant.

Existing alert transition projections gain an optional nullable acknowledgement
annotation, joined by exact phase version in the same set-based statements. Queue
and detail expose the current annotation through latest_transition; history
retains earlier annotations. Original phase/evidence/times stay immutable while
operator annotations can arrive later during traversal. Reuse the same receipt
wire projection for command and read annotations.

## Consequences

HTTP baseline.26 adds a guarded PUT and an optional read annotation, and expands
tenant discovery from eight to nine actions. Regenerate strict clients before
rollout/delegation; optional additive fields can still be rejected by old closed
response validators. Times/versions remain exact decimal strings. Browser
Origin/CSRF and fresh opaque-session validation protect commands and retries.

Migration 0048 creates empty immutable audit storage and expands small grant/audit
constraints, briefly locking those tables. It changes no existing state, grants
or receipts. Deploy schema-compatible binaries together and retain audit/history
on rollback; older checked-schema binaries reject a newer ledger. No dependency,
notification, silencing, assignment or UI capability is added.

## Alternatives considered

Reusing alerts.manage would require operators to receive unrelated rule-editing
power. Incrementing the phase-transition version would invent a condition change
without calculator evidence. Mutable or last-author-wins acknowledgement would
erase attribution on retries/races. Carrying an acknowledgement across changed
phase versions could hide a newly firing condition. Delivery suppression is a
separate product policy and remains outside this command.
