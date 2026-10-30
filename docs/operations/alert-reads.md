# Tenant alert reads

## TL;DR

HTTP baseline.25 exposes the retained alert queue, exact detail and bounded
transition history under current tenant `alerts.read`. Reads never execute a rule
or send a notification. See [ADR 0132](../adr/0132-publish-bounded-alert-reads.md).

## Routes and authority

Use the listener's existing workload bearer or browser session. Every request
rechecks current identity and the exact alerts.read grant before discovery;
metadata/telemetry/analytics/alerts.manage do not imply it. Suspended tenants are
forbidden; inactive identities cannot authenticate. All responses use no-store.

- GET `/api/v1/tenants/{tenant_id}/alerts` returns `{alerts,next_after}`. Omitted
  phase selects pending/firing. Optional `phase` is exactly inactive, pending,
  firing or resolved. Rule revisions with no phase transition are not alerts.
- GET `/api/v1/tenants/{tenant_id}/alerts/{rule_id}/revisions/{revision}` returns
  `{alert,definition}`. The alert identity includes the immutable rule revision,
  which is distinct from the lifecycle phase-transition version. Missing, foreign
  or never-transitioned records return 404. Query parameters are rejected.
- GET the same exact path plus `/transitions` returns `{transitions,next_after}`.
  Original phases, event/receipt instants, activation generations and evidence
  references are retained. An exhausted page is empty; an unknown alert is 404.

Both pages accept limit 1..100 (default 50) and opaque `after` continuations.
Unknown/duplicate parameters, malformed or cross-scope cursors and invalid limits
return 400. Path revision is canonical positive ASCII decimal u32 without leading
zeros. Times and positive BIGINT lifecycle versions are strings, retaining exact
values beyond JavaScript integer precision. Rule revisions remain JSON integers.

## Interpretation and evidence

Queue order is C-collated rule ID then rule revision. Phase and latest time are
mutable and do not define the cursor. Transition order is ascending lifecycle
version. These are traversals, not a snapshot or a recency-ranked queue: inserts
before a cursor require restarting and filter membership can change between pages.
Tokens bind tenant/resource/filter or exact rule revision, confer no authority and
allow page size changes. Changing filters/resources requires a fresh traversal.

`selected` means this revision is selected for processing. It is separate from
condition health: deactivated/superseded firing alerts remain visible. Inactive
filter results represent previously observed/cancelled lifecycles, while unused
configuration checkpoints stay hidden. Summary checkpoint and latest transition
come from one statement; exact detail also pins the original numeric rule grammar.

`latest_transition.evidence` and each history entry identify the exact evaluation,
semantic scope and output ordinal. Use these with the existing
[calculator evidence reads](calculator-evidence.md); those routes retain their own
telemetry.read requirement. A null output ordinal identifies missing/unusable
output evidence and does not mean false. Raw manifests, numeric outputs and
payloads are not expanded here. Latest-transition evidence can be older than
last_event_at_ms when later evidence left the phase unchanged.

## Rollout and rollback

Apply 0047 and deploy schema-compatible binaries together. Its partial observed-
checkpoint index uses explicit C ordering without rewriting facts/grants/state.
Measure retained volume and choose an idle deployment window because transactional
index creation can block worker updates. Regenerate clients from baseline.25.
Rollback stops affected traffic and retains schema/history; older checked-schema
binaries reject the newer ledger. Acknowledgement, alert SSE, aggregates, delivery
and connected UI remain later slices. No new dependency or permission is added.
