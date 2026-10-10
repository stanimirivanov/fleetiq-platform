# ADR 0075: Extract ingestion application capability

## Status

Accepted, 1 October 2026.

Acceptance reconciled from the approved architecture and harness sequence and
merged PR #206, commit `372a4d925b4e911eb3d375a2a42db279256e63e7`.
This records merge evidence; it does not claim independently enforced review
settings or infer acceptance from passing tests.

Supersedes: none. Superseded by: none.

## Context

The broad `fleetiq-application` crate owns accepted-message and decoded-result
durability orchestration, protocol release catalogue ports, and live projection
contracts. PostgreSQL imports ingestion ports through that broad crate. As a
result, ingestion changes and adapter compilation are coupled to unrelated
identity, metadata, and live application surfaces.

Accepted message identity, exact retry behavior, immutable collision handling,
decoded batch atomicity, and durability-gated replies must remain unchanged.

## Decision

Create `fleetiq-ingestion` as the application capability that owns the
accepted-message log, decoded-observation log, protocol-package catalogue,
frame acceptance, observation persistence, and their consumer-owned ports.
Move the existing implementations and tests unchanged. The crate depends on
domain values, trusted protocol-host policy, protocol SDK contracts, and
protocol-neutral telemetry values. It does not depend on adapters, access
policy, identity, metadata, or live projections.

PostgreSQL production adapters import these ports directly. Their dependency
on the broad application facade is limited to integration tests. The broad
`fleetiq-application` crate temporarily re-exports the same public API for
existing callers and integration tests. The exact dependency policy records
the package and its direct edges. This is a compile-time ownership change;
there is no database migration, wire-contract change, or new third-party
dependency.

## Consequences

Ingestion orchestration can evolve without an implicit dependency on unrelated
application capabilities. The trusted host still owns validation and
side-effect policy, while ingestion owns durable sequencing and retry
contracts. Existing application imports remain source-compatible until the
facade is retired. PostgreSQL behavior and persisted meaning are unchanged.

The application crate remains a temporary facade plus live contracts. Existing
PostgreSQL integration tests may use the facade while production adapters
target the defining crate. Rollback requires source changes only; no persisted
data or deployment migration is involved.

## Alternatives considered

Leaving ingestion in the broad application crate would retain the unwanted
compile-time coupling. Moving durability orchestration into the protocol host
would blur the distinction between protocol policy and application persistence.
Splitting each log into its own crate now would add package overhead without
independent consumers or deployment needs.
