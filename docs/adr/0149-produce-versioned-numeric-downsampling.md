# ADR 0149: Produce versioned numeric downsampling

## Status

Proposed, 6 November 2026.

## Context

M06 needs bounded numeric summaries without changing raw history, hiding unavailable
archive coverage, or silently rewriting results after late evidence. Existing exact
property history, work reservations and verified restore provide the source boundary.

## Decision

Add explicit trusted produce_numeric_summary_once composition. No startup worker,
HTTP summary API, calculator substitution or UI is added. Absent/disabled lifecycle
policy denies production; one turn reserves 2 MiB, has twenty seconds, and uses
two-second SQL statements/lock waits. Cancellation retains capacity until expiry.

Algorithm numeric-fixed-v1 uses epoch-aligned, half-open full buckets over at most
seven days, sixty-four buckets and one hundred unique complete interpretation keys.
Source selection has a 101-row look-ahead and never truncates an oversized input.
The reducer admits at most 116 raw hot/restored candidates before deduplication.
A sample means an interpretation (including binding revision and time basis), not
a deduplicated raw device observation. Corrections/time-basis choices are not collapsed.

Only integer and decimal property definitions are supported. The exact asset/type/
property-version membership is checked. Every sample must retain its declared kind
and canonical unit; no numeric kind coercion, conversion or interpolation occurs.
Only good quality contributes to count/min/max/sum. Uncertain/invalid qualities
remain in lineage and total/excluded counts. Empty and excluded-only buckets differ;
both have absent extrema, zero sum and an undefined mean. Mean is the exact sum/count
pair; no rounded decimal division is performed.

Reuse pinned BigDecimal 0.4.11 and the calculator's established coefficient-text
limit of 1024 characters and scale range -256 through 256, checked before arithmetic.
Normalize persisted values into plain exact decimal strings. Full Unicode identity
keys retain domain-valid identifiers; the 8192-byte encoded-key cap accommodates
their complete composite JSON representation.

A sealed repeatable-read capture includes property meaning, hot rows, current
admission and optionally one explicit ready restore file with verified digest/length.
A consumer-owned decoder port uses the existing exact archive codec and must return
the complete selected membership. Hot/restored overlap is compared by full identity
and original evidence, then deduplicated once. Missing/unready/cancelled staging or
reclaimed target evidence outside the selected manifest fails CoverageUnavailable.
Coverage describes hot_snapshot or hot_and_selected_manifest_snapshot, never all
source observations, all archives or permanent interval completeness. Later restore
expiry does not invalidate an already captured historical fact.

Persist an exact series key (tenant, target/version, window, width, algorithm and
optional manifest/request generation) and immutable revisions. Fingerprint canonical
JSONB but compare full content on retry; hash equality alone is not authority.
Unchanged evidence reuses its original revision/observation time. New late inputs
create another revision. A slower unpublished snapshot that drops previously
published input objects fails StaleSnapshot instead of regressing the head.
Already published older content still resolves to its original receipt.

Publication pins READ COMMITTED, locks the tenant/series and exact live current-policy
reservation, and commits the record/head together. SQL guards preserve scope,
initial/next revision, immutable records, exact bucket geometry, kind/unit, lineage
counts, explicit coverage and typed numeric strings. The complete active-tenant,
amount, policy and lifetime admission predicate is repeated at deferred commit.
The private database role and decoder host remain trusted for source authenticity
and exact arithmetic; SQL does not reimplement arithmetic or replace host validation.

## Consequences

Migration 0057 only creates new tables/functions/triggers, with no existing-table
scan, data backfill or raw-history mutation. Defaults start no summary jobs.
Historical snapshots remain derived evidence with original keys/values/quality;
raw history and calculator inputs are never automatically replaced.

The history application gains one declared edge to the already present BigDecimal
crate, dual MIT/Apache-2.0 licensed. No new package, version, feature or MSRV is added.
The alternative PostgreSQL-only arithmetic would couple core semantics to the
adapter and weaken small-test coverage. Persisted strings/profile remain owned by
history, providing an exit path under a separately versioned algorithm if needed.

Sparse windows still emit all requested buckets. Reclaimed coverage checks are
conservative at target scope, so an otherwise answerable interval may be refused.
Repeated runs are explicit refresh attempts; revisions order publications, not
source arrivals, and observed time is snapshot context rather than a completeness
watermark. A complete return can be retried; uncertain commits converge by full
content. Summary reads/API and installed/resource qualification remain later slices.

Rollback stops callers and retains the additive tables and old derived revisions.
No original evidence or existing HTTP contract changed. Do not drop immutable
summaries as a substitute for an operational retention policy.

## Alternatives considered

- Floating-point sums or implicit mean rounding: lose exactness and change semantics.
- Implicit archive scans: introduce unbounded I/O and ambiguous coverage.
- Mutating existing summaries for late arrivals: loses original lineage and results.
- Silent truncation or empty success on unavailable input: misrepresents evidence.
- Event-time-only deduplication: collapses distinct binding/time-basis interpretations.

## Verification

Pure tests cover signed boundaries, integer/decimal extremes, exact sums, extrema,
quality exclusions, Unicode keys, collisions and bounded numeric work. PostgreSQL
tests cover immutable/concurrent revisions, frozen inputs, stale publication,
expired/changed admission and malformed direct writes. Producer tests exercise
default-off behavior, target membership, inactive tenants and selected restored
coverage/overlap. Guard-removal experiments, full acceptance and final-commit
supply-chain verification are required before PR creation.

Private SQL writers must retain the initially deferred completion constraint
through commit; forcing it early and then changing admission is outside the
supported publication protocol. The producer owns its transaction and never
forces this constraint early. Database owners and the decoder host remain trusted.
