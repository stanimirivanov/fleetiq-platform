# ADR 0076: Extract live projection contracts

## Status

Proposed, 1 October 2026.

Supersedes: ADR 0008 (crate placement only). Superseded by: none.

## Context

ADR 0008 defines revisioned snapshots, deltas, continuity cursors, and
resnapshot reasons without choosing a transport. It placed those generic
contracts in the broad `fleetiq-application` crate. Identity, metadata, and
ingestion now have narrower capability crates, leaving the broad crate as a
compatibility facade plus this single live module. Consumers of projection
contracts should not inherit unrelated application dependencies.

The accepted continuity, authorization-change, and resnapshot semantics must
remain intact. There is still no production live projection store, transport,
or operator UI.

## Decision

Move the existing live module and its tests unchanged into `fleetiq-live`, a
transport-independent contract crate with no workspace dependencies. The
temporary `fleetiq-application` facade re-exports the same public types to
preserve existing import paths. Record the package and its direct facade edge
in the exact workspace dependency policy.

This refines only the crate placement chosen in ADR 0008. Its behavioral
contract and implementation obligations remain accepted. No database schema,
wire format, runtime service, third-party dependency, or deployment topology
changes.

## Consequences

Future projection producers, transports, and consumers can depend on a small
contract without importing identity, ingestion, or metadata use cases. Existing
callers keep their source-compatible facade path until it is retired. The
compiler now prevents the live contract crate from importing application or
adapter crates without a reviewed dependency-policy change.

The move has no data migration or runtime rollout. Rollback is a source-only
reversal. Production snapshot-to-stream handoff, authorization checks,
backpressure, persistence, and UI remain to be implemented.

## Alternatives considered

Keeping the contracts inside the broad facade preserves the avoidable
crate-level coupling. Moving them into domain would make projection transport
continuity appear to be a physical-asset invariant. Introducing a live
service or persistence port now would add unused runtime design before a
vertical live feature exists.
