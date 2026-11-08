# ADR 0157: Report lifecycle storage usage

## Status

Proposed, 8 November 2026.

## Context

The original storage response has a closed schema and explicitly marks
archive and export usage as unmeasured. Operators need lifecycle totals
that separate usable artifacts from expired retained bytes.

## Decision

Publish a companion operations.read endpoint in baseline.38. Reconcile
committed manifest lengths and disjoint retained export classes in one
deadline-bounded PostgreSQL snapshot. Use numeric aggregation and exact
decimal strings. Treat receipt-only jobs as zero retained storage.

## Consequences

Existing clients keep the original response contract. Archive totals describe
committed manifest evidence. Export totals describe retained logical artifact
lengths. The database remains the only source for these measurements.

## Alternatives considered

Changing the original fields would break clients that validate their literal
values. Provider listing would measure a different surface and introduce
availability and authorization dependencies into a bounded tenant read.

## Verification

PostgreSQL tests cover tenant isolation, exact reconciliation, expiry and
cleanup, repeated archive publication, and a statement deadline. The contract
tests verify exact string representation and the bundled response schema.
