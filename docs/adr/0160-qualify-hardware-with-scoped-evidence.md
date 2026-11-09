# ADR 0160: Qualify hardware with scoped evidence

## Status

Proposed, 9 November 2026.

## Context

FleetIQ exercises Teltonika decoding and installed ingestion with synthetic
traffic. M07 needs repeatable evidence for a physical device on a specified
configuration. A valid packet, a successful transport exchange, and an
operator-visible observation establish different parts of that evidence.

## Decision

Start with one owner-selected Teltonika target within the implemented TCP Codec 8
and Codec 8 Extended subset. Bind qualification to the exact device model,
hardware revision, firmware, settings profile, codec and deployment under test.
The initial target is pending selection. The operator supplies an isolated
private ingress path and authorizes capture before the first device run.

Use the [hardware qualification contract](../../contracts/hardware/README.md)
for fixture provenance, evidence custody, run outcomes and support decisions.
Require independently reviewed expectations and matching laboratory and field
evidence for each supported tuple. Freeze the required cases and field coverage
before accepting campaign results.

Platform owns wire expectations and the versioned evidence contract. Deployment
owns installed runs, fault scenarios and report assembly. The existing UI
provides the operator's view of stored observations and provenance.

## Consequences

An early physical tracer establishes access and tests the evidence collection
path. Full qualification requires the later laboratory and field campaigns.
The owner must supply the target, operator, ingress, custody and field criteria.
Until those prerequisites are satisfied, the target has incomplete evidence.

Throughput is descriptive for a declared workload and environment. A performance
threshold requires a separately approved workload, resource envelope and limit.

## Alternatives considered

A protocol-wide claim based on synthetic replay would obscure differences
between firmware and settings. Storing original captures in the fixture corpus
would distribute sensitive device data. Exact target claims and private capture
custody address those risks while preserving reproducibility.

## Verification

Repository policy checks the ADR registration and documentation links.
Contract validation, physical execution and report evaluation are subsequent M07
deliverables.
