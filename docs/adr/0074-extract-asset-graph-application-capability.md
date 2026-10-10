# ADR 0074: Extract asset graph application capability

## Status

Accepted, 1 October 2026.

Acceptance reconciled from the approved architecture and harness sequence and
merged PR #203, commit `d2ba1649f35cf01a7995dbae195b965b22e0e1c7`.
This records merge evidence; it does not claim independently enforced review
settings or infer acceptance from passing tests.

Supersedes: none. Superseded by: none.

## Context

The broad `fleetiq-application` crate still owns metadata use cases beside
ingestion orchestration and live contracts. Its asset, definition, device,
relationship, and signal-binding ports are consumed by metadata services, but
PostgreSQL imports them through the broad facade. This leaves an avoidable
crate-level path from metadata code to unrelated ingestion behavior.

The existing tenant authorization, temporal query contracts, PostgreSQL
semantics, and callers must remain stable during extraction.

## Decision

Create `fleetiq-metadata` as an application capability for tenant-owned
definitions, assets, communication devices, relationships, and signal bindings.
Move each use case, its consumer-owned repository port, and its tests unchanged.
The crate depends on `fleetiq-access` for action-typed authorization evidence,
`fleetiq-domain` for validated values, and `fleetiq-semantics` only for the
already policy-owned signal-binding snapshot query.

PostgreSQL production adapters import metadata ports directly.
`fleetiq-application` temporarily re-exports the public metadata API for
existing callers and integration tests. The exact workspace dependency policy
records the new package and edges. This is a compile-time boundary inside the
existing backend process; no database migration or external dependency is
required.

Repository interfaces stay consumer-owned and specific to their use cases.
The definition repository's publication and exact-lookup methods are all
consumed by one definition service; splitting that interface during this
ownership move would add adapter churn without a second consumer. Semantic
resolution already has separate narrow read ports owned by semantic policy.

## Consequences

Metadata use cases cannot import ingestion or live application modules without
a new reviewed dependency. PostgreSQL continues to implement the same trait
contracts, and existing `fleetiq_application` imports remain source-compatible
until callers migrate. Authorization, tenant ownership, revision append,
bitemporal selection, and validated reconstruction are unchanged.

The private authentication fixture moves into the metadata crate for its
tests, preserving the public evidence-construction boundary. PostgreSQL
integration tests may continue using the compatibility facade during this
extraction; they should move to defining crates when the facade is retired.
Rollback requires source changes only.

## Alternatives considered

Keeping metadata inside the broad application crate leaves its dependency
boundary conventional rather than compiler-enforced. Moving semantic read
ports into metadata would reverse their consumer ownership and recreate a
policy-to-application dependency. Splitting the definition repository into
small traits now would have no independent consumer and would obscure this
behavior-preserving extraction.
