# ADR 0120: Explore bounded semantic graphs

## Status

Proposed, 27 October 2026.

Supersedes: none. Superseded by: none.

## Context

The semantic inspector needs a connected explanation of assets, physical
relationships and signal attribution. Independent per-asset pages can cross
commit snapshots, multiply round trips, resurrect old endpoints after a
correction, and expand without a bound on legal cycles or high fan-out. Existing
PostgreSQL revisions and access paths already represent the required facts; a
graph datastore is not needed for this initial bounded read.

## Decision

Metadata owns the graph query, validated budgets, breadth-first coordinator and
consistent-read ports. PostgreSQL owns the read-only repeatable-read session,
set-based frontier queries and validating row mapping. HTTP authenticates,
obtains fresh typed `ReadMetadata` evidence and maps the application result.
No telemetry values, raw payloads, external identifiers or device secrets are returned;
existing asset summaries, relationship and binding wire schemas are reused.

Publish GET `/api/v1/tenants/{tenant_id}/assets/{asset_id}/semantic-graph` with
mandatory effective/known cutoffs and optional budgets. Root depth is zero.
Defaults are depth 2, 50 unique assets, 100 relationships and 100 bindings;
ceilings are depth 4, 100 assets and 200 links of each kind. Depth zero is valid;
other budgets are positive. Traverse adjacency in both directions, retaining
the directed source/target and pinned type/property references in the response.
Binding adjacency joins its source device asset to its target asset; this is
configured attribution, not observed sensor inventory.

Consider relationships before bindings per breadth-first layer. Queries order
identities with PostgreSQL C collation; output arrays order unique identities.
Visit assets once. Self-links, legal cycles and shared descendants remain
representable without recursion. A boundary layer is inspected to include
links between already included assets and to detect outward depth omissions.
Every included relationship/binding has both endpoint assets in the response.
A node-budget or depth rejection omits the link instead of returning dangling
references. Selection order is deterministic, not a business-priority ranking.

Each layer performs at most two adjacency queries, independent of node count.
Each returns only the remaining kind budget plus one sentinel, excluding
already included identities. A sentinel reports relationship/binding prefix
truncation. Node/depth omissions have separate flags. These conditions may
coexist, and rejected links can leave result counts below the budget even when
a prefix was truncated. A bounded graph is not necessarily an induced subgraph.
There is no continuation cursor or full-tenant inventory claim.

Select each latest known revision before endpoint/effective filters, so
corrections cannot resurrect earlier adjacency. All root, frontier and final
asset reads use one read-only repeatable-read transaction with pinned cutoffs.
At most five layers plus two asset batches produce twelve SELECTs. Use existing
endpoint/target revision indexes and a two-second per-statement timeout. The
HTTP graph future has an eight-second total deadline; timeout/error returns a
safe failure, never an invented empty or apparently complete partial graph.
Dropping the session rolls back and restores transaction-local settings.

## Consequences

No dependency, migration or new index is required. Query/output cardinality and
application memory are bounded; LIMIT does not prove bounded candidate scans
or constant query cost. Large revision histories may time out and need measured
index/read-model improvements rather than larger silent limits.

A missing root is 404 after authorization. A known isolated root is a complete
single-asset graph. Asset summaries use the immutable current catalogue; their
existence is not reconstructed as of the cutoffs. Effective/known time applies
to revisioned relationships and bindings. Grants and identity are always current.

An empty truncation list means complete reachable adjacency in this committed
database snapshot. Separate requests can see later commits, including backdated
metadata revisions, even with identical cutoffs. Clients expand by choosing
another root or bounded budgets; do not combine responses into an asserted
single atomic graph. Current telemetry, history, health and physical truth are
separate capabilities. D06/full-profile qualification remain deferred.

## Alternatives considered

- Unbounded recursive SQL or a graph database: unnecessary for this bounded
  capability and harder to express independent work/output limits.
- One existing page lookup per vertex: rejected because round trips scale with
  node count and independent reads can mix snapshots.
- Traverse only physical relationships: insufficient for explaining source
  devices that attribute to subsystems without a physical relationship edge.
- Return omitted endpoint IDs as complete edges: rejected because consumers
  would receive a structurally inconsistent graph.

## Verification

Pure application tests cover cycles, self-monitoring, depth/asset/link limits,
closed endpoints, missing roots, failure classification and bounded batch counts.
PostgreSQL tests cover high fan-out, shared identities across tenants, moved
relationship/binding corrections, half-open applicability, a concurrently
committed backdated edge and a controlled table-lock timeout. HTTP tests cover
schema/example parity, mandatory cutoffs, invalid budgets, exact metadata
permissions, revocation/suspension, safe failure and no-store. Full runtime,
database and workspace gates run in CI; focused compile/lint and structural
contract checks run locally without claiming complete acceptance.
