# ADR 0069: Own semantic read ports in policy

## Status

Proposed, 30 September 2026.

Supersedes: ADR 0003 (blanket adapter placement only). Superseded by: none.

## Context

ADR 0003 placed focused binding and property-definition read traits in
`fleetiq-semantics`, but used blanket implementations for application-owned
repositories. That made inward semantic policy depend on the broad application
crate. ADR 0066 recorded this exact edge as a temporary exception: future
application orchestration could not depend on semantic resolution without a
Cargo cycle.

The existing bitemporal query is also declared in application, even though
semantic resolution consumes its two time cutoffs. PostgreSQL already owns the
SQL queries that can serve both application repositories and the narrower
semantic views. No persisted schema or resolution rule needs to change.

## Decision

Move `SignalBindingSnapshotQuery` into `fleetiq-semantics` beside its
consumer-owned `SignalBindingView` and `PropertyDefinitionView` traits. Remove
the blanket implementations over application repositories. Implement both
views directly on the existing PostgreSQL adapter types, delegating to their
validated read methods and preserving their tenant, absence, error, and
bitemporal semantics.

Application continues to expose the query through a public re-export, so
existing application repository and service callers use the same type and
constructor. The direct Cargo edge becomes `fleetiq-application ->
fleetiq-semantics`; PostgreSQL depends on both. Remove the temporary reverse
edge and its architecture-policy exception.

All resolution rules, typed outcomes, and provenance decisions from ADR 0003
remain in force. This decision replaces only the blanket-adapter placement
that created the reverse dependency.

## Consequences

Application orchestration can now consume semantic policy without a Cargo
cycle. A semantic read view can be backed by PostgreSQL or a worker-local
projection without implementing unrelated application write operations. The
application re-export preserves the current Rust source-level query API; the
type's defining crate changes, so downstream code naming the type in advanced
traits or documentation should use the semantic owner over time.

PostgreSQL implements two traits with the same method names as existing
application repositories. Implementations call explicit internal methods;
callers that import both traits may need qualified syntax. SQL, migrations,
wire contracts, and runtime composition remain unchanged.

## Alternatives considered

Retaining the blanket implementations would preserve the reverse dependency
and block application-to-semantics orchestration. It would also make every
application repository an implicit semantic adapter, including repositories
whose read semantics were not reviewed for that use.

Duplicating the snapshot query in both crates would avoid an application
dependency but create two bitemporal contracts that could diverge and require
conversion at each adapter boundary.

Moving semantic read traits into application would weaken consumer ownership
and make the resolver depend on broader application concerns.
