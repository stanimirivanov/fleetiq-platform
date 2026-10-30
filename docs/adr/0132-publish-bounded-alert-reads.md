# ADR 0132: Publish bounded tenant alert reads

## Status

Proposed, 30 October 2026.

Supersedes: none. Superseded by: none.

## Context

[ADR 0131](0131-evaluate-durable-alert-lifecycles.md) retains revision-local
checkpoints and exact evidence-linked transitions. M05 needs a protected queue,
exact detail and bounded history before acknowledgement or connected UI work.

## Decision

Publish read-only queue, detail and transition pages beneath
`/api/v1/tenants/{tenant_id}/alerts`. Require independent current `alerts.read`
before parsing feature parameters or discovering resources. Existing bearer and
browser-session modes share composition; other grants do not imply this action.
Use existing bounded problem responses and no-store on successes and failures.

Identity is the tenant plus rule ID and immutable rule revision, with semantic
scope pinned by that definition. Only checkpoints with a positive transition
version are observed alerts: never-transitioned configuration revisions are not
listed and exact reads return 404. Queue omission of phase selects pending/firing;
an explicit inactive/pending/firing/resolved filter inspects retained state.
Deactivated or superseded firing alerts remain visible, with a separate selected
boolean; selection is not evidence that a condition resolved.

Queue traversal uses C-collated rule ID then rule revision, excluding mutable
phase/time from the position. History uses increasing positive transition version.
Limits default to 50, range 1..100, and query limit+1 supplies continuation. Reuse
FleetIQ's bounded opaque cursor codec; scope its position to tenant/resource/filter
or exact revision. Tokens grant no authority. Page size can change. Pages are
traversal, not a live snapshot: rows can change membership, earlier inserts require
restarting, and later immutable transitions can enter subsequent history pages.

Project each summary's checkpoint and latest transition in one SQL statement.
Detail adds the exact typed pinned rule grammar; it reuses the existing wire
projection rather than exposing database JSON. History retains original event and
database receipt times, phase version and activation generation. Serialize signed
millisecond instants and positive BIGINT lifecycle versions as decimal strings,
separately from u32 rule revisions, preserving values beyond JavaScript precision.

Link exact calculator evidence by evaluation ID, scope asset ID and selected
output ordinal. Null ordinal explicitly indicates unknown evidence without a
usable output slot. These references do not copy raw manifests/outputs, do not
recalculate state, and do not confer the separately required telemetry.read grant
on the existing calculator evidence routes. Latest phase-transition evidence can
precede the checkpoint's latest event when subsequent evidence did not change phase.

## Consequences

HTTP baseline.25 adds three GET operations without changing existing action names,
mutation semantics or dependencies. Migration 0047 adds a partial C-collated queue
index for observed checkpoints only; it changes no facts, grants or selections.
Transactional construction can block lifecycle updates, so measure retained volume
and use an idle deployment window. Deploy compatible checked-schema binaries and
retain additive schema/history on rollback; older binaries reject newer ledgers.

No acknowledgement, SSE, aggregate dashboard, network delivery or UI is added.
The queue does not promise global recency order, a fixed snapshot, or raw evidence
access. Existing historical rule revisions stay inspectable without reactivation.

## Alternatives considered

Adapt existing FleetIQ bounded list/detail/evidence and cursor patterns without
an external dependency or interface wrapper. Mutable last-event ordering could
move an alert behind/ahead of a cursor and duplicate it. Eager history expansion
would make detail unbounded. Treating rule selection as health would hide unresolved
firing conditions. Implicit telemetry authority would weaken the existing boundary.
