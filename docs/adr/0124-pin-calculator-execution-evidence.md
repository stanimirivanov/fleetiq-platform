# ADR 0124: Pin calculator execution and evidence

## Status

Proposed, 29 October 2026.

Supersedes: none. Superseded by: none.

## Context

[ADR 0007](0007-calculator-output-contract.md) defines host-owned evaluation and
output identity, but not immutable configuration, input selection or late data.
M05 persistence and workers need one exact replay contract without introducing
storage, scheduling or algorithm dependencies into analytics policy.

## Decision

Add immutable configuration and evidence identifiers using the existing opaque
identifier validation. Configuration identity binds immutable content: input
roles, exact definitions, canonical units and algorithm parameters. The durable
host must reject reuse with different content. Evidence identity names a full
immutable semantic fact, not an asset/property latest-value row.

A window is a nonempty half-open event-time interval of at most seven days.
Its explicit finalization time is the end plus allowed lateness, from zero to
seven days. Checked subtraction rejects overflow. These are initial bounded
contract limits, not claims about retention or scheduling guarantees.

A manifest carries that window, one recorded-time knowledge cutoff at or after
finalization, and at most 1024 exact evidence references. It rejects duplicates
regardless of timestamp and facts outside the interval. Canonical ordering is
(event time, evidence identity); overflow fails rather than truncates. Empty
selection is valid, but does not establish sufficient evidence or a healthy value.

The host must read a consistent snapshot and verify each reference's tenant,
content, event time, configuration role and visibility at the cutoff. These
cross-record invariants cannot be proven by an in-memory reference constructor.
The host must also prove selection completeness within the declared configuration;
this contract does not authorize accepting only the first 1024 rows.

The logical execution key includes tenant, scope asset, exact calculator release,
immutable configuration identity and the complete manifest. It excludes processing
time and attempt number. Durable orchestration maps an exact key to one existing
evaluation ID and reuses that ID, first evaluation context and ordered equivalent
outputs on retry. The existing output assembly API and its zero-based positions
remain unchanged. When assembling its evaluation context the host uses the
manifest window end as logical event time; processing time remains separate.

Late evidence after the pinned cutoff cannot alter an existing evaluation.
A corrected selection/configuration/cutoff creates a distinct key and evaluation,
linked by future persistence as a correction rather than an overwritten fact.
Historical correction must not implicitly authorize new external notifications.

Terminal outcomes distinguish complete (including zero outputs), insufficient
evidence and bounded failures: incompatible input, invalid output or resource
limit. Only complete evaluations may publish outputs. Future atomic persistence
must enforce that relationship. Storage availability and expired leases are host
retry conditions, not terminal calculator failures or successful empty results.
No raw exception text or input values belong in failure classifications.

## Consequences

Two later consumers share these contracts: immutable evaluation persistence and
the bounded one-shot/background runner. No runtime, serializer, dependency,
migration or HTTP behavior is added. ADR 0007 output identities remain compatible.
Exact IDs and bounded manifests support replay comparison; they are not hashes,
storage keys, authenticity proofs or a substitute for database tenant constraints.

## Alternatives considered

Using processing time as replay identity duplicates outputs on retry. Mutable
latest-value inputs cannot reconstruct an earlier result. Silent truncation
changes algorithm meaning. Unbounded manifests and free-form errors expose cost
and disclosure risks. A general stream processor or plugin ABI is unnecessary
for this first contract slice.
