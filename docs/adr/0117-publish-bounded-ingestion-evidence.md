# ADR 0117: Publish bounded ingestion evidence

## Status

Proposed, 26 October 2026.

Supersedes: none. Superseded by: none.

## Context

A received frame, a decoded result, a replay lease and a live connection are
separate facts. An operator needs safe source discovery and diagnostic evidence
without an unbounded history scan or access to protocol bytes. Device silence
cannot establish worker failure, nor does a lease prove that its owner is alive.

## Decision

The ingestion capability owns an authorized diagnostic service and read port.
HTTP requires fresh tenant `protocol.manage`, using either configured browser
sessions or workload authentication. PostgreSQL returns a read-only repeatable
snapshot; the transport exposes closed DTOs with no values, raw payload, session
state, external identifier claims or credentials. Existing package and route
reads remain the authorities for registration, admission and configuration.

Inspect at most 64 newest accepted messages ordered by host receipt time and
message identity, using an additive tenant/device ordered index. A 65th receipt
is only a truncation sentinel. Exact-key joins expose completion and scheduling
evidence. Inspect at most 64 decoded positions per selected message before
source deduplication, then return at most 128 distinct package/version/endpoint/
signal tuples. Report truncation separately for receipts, output prefixes and
source tuples. These are observed sources in a sample, never a capability
inventory. A two-second per-statement database timeout bounds lock/plan waits.

Expose accepted, queued, leased, expired, poisoned and decoded evidence. Empty
decoding is completed work. Lease classification uses the snapshot database
clock. Connection and worker liveness remain explicitly `not_observed`: there
is no cross-process heartbeat collector, and historical receipt age is not a
connectivity or asset-health assertion. Clients may show receipt age and
processing evidence separately without manufacturing online/offline health.

The measured fixture exercises real sockets, live policy, transactions and
backpressure with a deterministic decoder. It records its excluded Wasm/child/
vendor costs, workload and resource observations. Functional deadlines and
cardinality/admission assertions are gates; uncalibrated throughput or RSS
numbers are not portable performance SLAs. Existing isolated process and vendor
conformance coverage remains required separately.

## Consequences

The API is additive and bounded independently of history size. Large devices
need later explicit discovery/indexing requirements rather than silent widening
of this sample. Snapshot reads do not fence a later mutation. Receipt timestamps
may be equal, historical or out of order; ordering is deterministic but not an
insertion/commit chronology. The index build blocks writes under normal
transactional PostgreSQL DDL; schedule deployed migration accordingly. Runtime
schema checks require migration 0034 before serving this binary.

## Alternatives considered

- Full-history DISTINCT/aggregation: rejected because LIMIT after aggregation
  does not bound input work.
- Persisting duplicate source inventories now: deferred until complete discovery
  and retention semantics justify projection maintenance and historical backfill.
- Inferring process/socket health from receipt age or a lease: rejected because
  idle devices and dead lease owners make those inferences unsound.

## Verification

Real PostgreSQL fixtures cover status/expiry, empty completion, tenant isolation,
bounded history/output/source sampling, consistent ordering and public redaction.
HTTP fixtures verify authentication, revocation, no-store, absence and schema
examples. The measured TCP fixture proves two admitted peers, excess rejection,
one durable completion per sent frame and zero pending work with aggregate
measurement output. A ten-second sampling window and 512-frame cap bound load
without requiring an uncalibrated fixed frame rate.

A decoded marker does not erase pending or poisoned replay evidence: legacy
result persistence can precede checkpoint processing. Diagnostic processing
state prioritizes outstanding/poisoned replay; `observation_count` independently
reports the existing decoded marker. It never converts that combination into
successful worker completion.
