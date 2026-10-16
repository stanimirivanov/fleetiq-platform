# ADR 0108: Expose attributed relationship and signal-binding revision commands

## Status

Proposed, 16 October 2026.

Implementation follows approved delivery-plan PR 10; acceptance awaits human review.

Supersedes: none. Superseded by: none.

## Context

Relationships and bindings already have immutable revision histories, stable
serialized heads, tenant-safe references and exact authorized reads. Operators
need to commission and explicitly correct these facts through HTTP without
turning physical effective time into recorded time or overwriting prior meaning.
Strict historical appends have no trustworthy actor attribution.

## Decision

Add POST alongside the existing exact relationship/binding GET routes:
`/api/v1/tenants/{tenant_id}/relationships/{relationship_id}/revisions/{revision}`
and `/api/v1/tenants/{tenant_id}/signal-bindings/{binding_id}/revisions/{revision}`.
Publish both unchanged GET representations in baseline.12.

The path selects a positive-u64 revision using ASCII decimal text, preserving the
existing parser's leading-zero acceptance; responses normalize to decimal text.
Revision 1 creates a lineage. Only the exact successor appends a correction.
The request contains complete type/endpoints or source/target plus a half-open
effective interval, never tenant, actor, revision identity or recorded time.
Omitted and null effective ends are equally unbounded.

Consume exact current tenant metadata.manage and derive actor attribution from
it. Independently require metadata.read for reads. Reuse the listener's selected
browser/workload identity and exact Origin/CSRF mutation guard. Authenticate and
authorize before reporting body/reference errors. Bound JSON at 32 KiB and reject
unknown and duplicate nested fields. Domain constructors validate local values;
existing composite foreign keys and target-property membership enforce references.

Metadata owns a narrow attributed revision port and use case. PostgreSQL extends
existing append functions to accept a caller-owned transaction, preserving their
sequence validation, stable-head locks and fresh post-lock dependent reads.
The strict ports keep their existing behavior. New commands atomically append
the fact, advance the head and insert original actor attribution.

Migration 0030 adds separate relationship and binding attribution tables with
composite exact-fact foreign keys, actor references and immutable update/delete/
truncate guards. Recorded time remains solely on the existing immutable fact,
avoiding two competing clocks. Empty additive tables require no backfill/rewrite;
installing foreign keys briefly locks referenced tables.

Return 201 appended with original actor/time, or 200 already_appended when the
same exact revision has equal complete content and attributed evidence. Compare
all client-selectable content, excluding newly sampled host time. Roll back a
sequence loser before fresh exact-fact and attribution reads. Both are immutable;
a later correction cannot invalidate an old retry. Different content, sequence
gaps/exhaustion, recorded-time regression or an unattributed legacy fact return
409. Missing same-tenant references return 404; storage/commit errors remain
ambiguous 500, never success. Responses are no-store and redact storage details.

## Consequences

An explicit revision acts as both concurrency expectation and exact retry key.
Concurrent identical requests create one attributed fact; different same-revision
content conflicts. Advancing another stable identity remains independent.
Corrections replace the complete known fact, not only an interval segment. An
actual physical handover uses separate stable identities with adjacent intervals;
this slice does not make multiple identities atomic as one batch.
A successful correction preserves older exact facts and inclusive known-time,
half-open effective-time snapshot selection. Host clock regression can temporarily
reject a successor; operators retry the same content after correcting clock health.

Legacy strict revisions remain readable but cannot claim attributed command
success. Appending their next revision creates new evidence without fabricating
earlier authorship. These commands do not enforce graph-wide cycles/cardinality,
verify decoder signal emission, trigger historical reprocessing, delete facts,
or create an ontology editor. Existing target-property membership is enforced.

Rollback uses a follow-up build retaining migration 0030 and its evidence while
removing commands. A pre-0030 binary rejects the newer applied migration absent
from its embedded inventory. Do not rewind the ledger or weaken validation.

## Alternatives considered

Editing the old revision destroys bitemporal lineage. Choosing an implicit next
revision makes a lost-response retry append a second correction. Client-controlled
recorded time permits forged history. Comparing newly sampled host time makes
ordinary retries conflict. Duplicating append SQL risks divergence in locking
and sequence rules. Replacing strict interfaces would unnecessarily change
internal consumers. Inventing actor/time for legacy facts is not audit evidence.
