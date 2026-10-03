# ADR 0092: Process one exact durable decoded observation

## Status

Accepted, 3 October 2026.

Supersedes: none. Superseded by: none.

## Context

ADR 0091 composes semantic resolution and immutable history append for a
decoded observation already in memory. Decoded observations are also durable
ingestion facts, but there is no application operation that can identify one
of those facts and submit it to the processor. A future worker must be able
to act on a durable identity without trusting a caller-supplied reconstruction
or loading an unrelated raw wire payload.

## Decision

`fleetiq-history` owns an exact decoded-observation read port and an
application operation that uses it before invoking the existing semantic
history processor. The request names tenant, accepted message, immutable
decoder release, output index, knowledge cutoff, and event-time basis. The
request is borrowed, so a lookup failure leaves the complete work identity
with its caller. Exact absence returns `None`; an unresolved interpretation
remains a present result. A view that returns a different tenant or output
identity fails closed before semantic resolution. Once loaded, processing errors retain the owned
observation or exact semantic change under ADR 0091.

The PostgreSQL adapter selects the observation and parent message metadata in
one query. It does not fetch the raw accepted-message payload. Tenant, decoder
release, and output index are exact predicates with no fallback. Database rows
are reconstructed through validated Rust types. Existing immutable source
facts and the history append adapter remain the authoritative lineage checks.

## Consequences

A caller can execute or retry semantic processing from a durable output
identity without supplying telemetry values. Missing source data remains
distinct from storage failure and unresolved attribution. An exact retry after
successful append remains idempotent. This operation does not enumerate work,
claim it, schedule a worker, or store unresolved outcomes; those require
separate policies and operating behavior.

This adds a checked SQL read and no migration or external dependency. The
query is one observation at a time; throughput and batching should be
measured when a production worker is designed.

## Alternatives considered

Reading the entire accepted message and decoding it again would re-run a
potentially different execution environment and fetch raw bytes unnecessarily.
Passing an in-memory observation to the history processor remains supported,
but cannot by itself verify which durable output a worker selected. Adding a
polling queue here would mix exact execution with scheduling and recovery
policy before those semantics are defined.
