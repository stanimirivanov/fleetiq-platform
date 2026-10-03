# ADR 0093: Read one exact durable semantic interpretation

## Status

Accepted, 3 October 2026.

Supersedes: none. Superseded by: none.

## Context

ADR 0090 makes semantic property changes immutable and gives each one a
source-output, binding-revision, and event-time-basis identity. ADR 0092 can
process a durable decoded output, but callers cannot read the resulting history
fact with its source and target provenance. A later operator query or projector
must not silently re-resolve an old change against a corrected binding.

## Decision

`fleetiq-history` owns an exact `AssetPropertyChangeView` read port. The request
supplies tenant and the complete semantic interpretation identity. Its result
contains the durable decoded observation, the exact target selected by the
stored binding revision, the selected event time, and the resolution cutoffs
recorded on first append. Absence is distinct from storage failure. The stored
read model validates that the observation identity and chosen event time agree
with the interpretation key.

PostgreSQL selects the exact history row and immutable binding target with
checked SQL, then reuses the validated decoded-observation mapper. These are
two point reads over immutable rows; a missing referenced source is corruption,
not ordinary absence. No read falls back to another tenant, decoder release,
output, binding revision, or event-time basis. The first-append knowledge cutoff
is evidence, not a live as-of filter or proof that the value is current.

## Consequences

An internal caller can retrieve and inspect one durable interpretation after
an exact append or retry. Binding corrections and different timestamp policies
remain separate readable facts. The read does not choose a winning or latest
value, enumerate history, enforce caller authorization, or expose HTTP. Those
require separate product and query policies. This change adds one checked SQL
query and no migration or external dependency. A future bounded page must use
set-based reads rather than invoking this point lookup once per item.

## Alternatives considered

Re-running semantic resolution on read would replace historical attribution
with today's metadata and lose correction provenance. Copying value and target
into the history table would duplicate immutable source facts. A broad history
page would require ordering, cursor, authorization, and index contracts that
are unnecessary for the first exact read.