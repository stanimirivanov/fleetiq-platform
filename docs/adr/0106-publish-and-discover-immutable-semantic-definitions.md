# ADR 0106: Publish and discover immutable semantic definitions

## Status

Proposed, 15 October 2026.

Implementation follows approved delivery-plan PR 8; acceptance awaits human review.

Supersedes: none. Superseded by: none.

## Context

Semantic definitions already have validated domain aggregates, strict persistence,
exact version reads and immutable PostgreSQL guards. Product commissioning needs
HTTP publication and discovery without changing existing meanings. Lost responses
and concurrent equal publications must resolve safely, while changed content
under an occupied version must be rejected.

## Decision

Expose collection GET/POST and per-identity version GET for asset types, properties
and relationship types. Keep all existing exact GETs and strict insert interfaces.
Reads consume exact metadata.read; writes consume exact metadata.manage. Derive
tenant ownership from consumed authorization, authenticate before revealing input
errors and reuse listener-selected browser Origin/CSRF protection.

A definition's tenant, kind, ID, numeric version and complete immutable content
identify publication. Return 201 for a new version, 200 for identical existing
content, and 409 for changed content. Compare property name, kind and unit; asset
name and ordered exact references; relationship-type name. No partial aggregate
may remain after a failed reference. Positive uint32 versions may have gaps.

Metadata owns a narrow publication port and a catalogue port alongside a closed
namespace-aware identity vocabulary. PostgreSQL implements them on the existing
definition adapter. Publication reuses strict atomic inserts and recovers only
the selected aggregate primary-key violation. PostgreSQL uniqueness resolves a
concurrent winner before reporting that collision. Compare the winner through
fresh statements; immutable rows and complete child membership remain protected.
Unrelated uniqueness, reference, check and storage failures are not retry success.
No migration or dependency is needed.

Catalogues return bounded reference/name summaries ordered by (ID, numeric version).
Use one tenant/namespace-scoped UNION query with optional exact-ID filtering and
one lookahead row; never load full child aggregates per summary. Cursors are
exclusive value positions, not snapshots, totals or latest-version aliases.
Limits are 1..100, default 50. Missing version catalogues are empty.

Reject unknown and duplicate input fields, bound JSON to 16 KiB and asset-type
membership to 100, and return safe no-store responses. Publication requires
existing referenced property versions within the same tenant. Omitted unit
means null; only numeric property kinds accept a unit. Domain constructors
remain authoritative for semantic invariants.

The key identifies immutable meaning, rather than an administrator command.
Different authorized publishers may resolve identical content. Attributed
publication audit is not introduced; do not fabricate publisher/time for earlier
definitions that lack that provenance.

## Consequences

Commissioning can discover and publish exact semantic versions through the existing
identity boundary. Baseline.10 adds six paths and nine operations without changing
previous payloads. Existing assets and bindings keep pinned meanings. The bounded
summary path avoids query-per-row loading and leaves complete content to exact reads.

A duplicate HTTP publication deliberately performs a failed strict insert followed
by content reads. Publication is low volume; preserving existing strict semantics
and simple fresh-snapshot correctness is preferred to replacing established SQL
in this slice. Later optimization must preserve winner visibility and complete
aggregate equality.

Rollback removes additive routes and retains immutable versions. UI editors,
definition deletion/editing, actor/time publication audit, asset/device registration,
ontology cardinality constraints and deployed-provider qualification remain separate.

## Alternatives considered

Treating every duplicate as success would hide changed meaning. Comparing a
conflicting insert within the same SQL snapshot can miss the concurrent winner.
Silently relaxing existing strict insert calls would change their consumers.
Loading every complete aggregate for catalogue pages would create N+1 queries.
Global metadata grants or request-selected tenants would weaken existing ownership.
