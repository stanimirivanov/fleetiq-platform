# Immutable semantic definition publication and discovery

Status: Implemented and locally verified; awaiting review and merge, 15 October 2026.

## TL;DR

Deliver original plan PR 8 in M02 - Product Foundation. Publish asset, property
and relationship types and discover their immutable versions through bounded
tenant-scoped HTTP catalogues. Preserve exact reads and legacy strict writes.

## Scope and contracts

Each namespace (asset-types, properties, relationship-types) gains collection
GET/POST and GET of one identity's versions. Collection and version reads return
bounded ID/version/name summaries; existing exact-version GETs retain complete
aggregate payloads. Publish additive OpenAPI baseline.10 with nine operations.

Publication consumes exact metadata.manage evidence; catalogues consume exact
metadata.read evidence. Neither action implies the other. The host derives tenant
scope from consumed authorization. Browser commands reuse exact Origin and
X-FleetIQ-CSRF protection; workload authentication remains a separate listener mode.
Reject unknown and duplicate fields, cap bodies at 16 KiB and property membership
at 100 references, and return safe no-store outcomes.

## Publication and concurrency

Tenant, kind, ID, version and complete immutable content determine the result:
201 published, 200 already_published, or 409 conflicting content. Names, property
value kind, optional canonical unit and ordered property references all matter.
Versions are positive uint32 and need not be contiguous. Missing exact referenced
property versions return 404 and leave no partial asset-type definition.

Keep existing strict insert APIs unchanged. The publication adapter reuses their
atomic writes, recovers only the selected aggregate's exact primary-key collision,
then reads the complete winning immutable aggregate in a fresh statement. PostgreSQL
uniqueness waits for concurrent winners; a later statement avoids the stale-snapshot
trap of selecting a conflicting insert in the same statement. Other constraints
and infrastructure errors remain failures. Existing immutable triggers protect
committed values; retries never update or reapply older meaning.

The publication key identifies content rather than an administrator command.
Another currently authorized publisher can resolve identical content. Actor/time
publication audit is not added and no historical attribution is fabricated for
definitions created through existing strict APIs.

## Discovery and boundaries

Identity and numeric version form the exclusive cursor. One checked query selects
the namespace and tenant, optional exact identity and at most limit+1 summaries.
No child-membership loading, counts or per-summary queries. Limits are 1..100,
default 50; missing IDs return empty pages. Cursors are traversal positions,
not snapshots, latest-version aliases or authorization evidence.

Metadata owns validated commands, discovery values and consumer ports. PostgreSQL
owns persistence, collision recovery and summary mapping. Server modules own
transport validation, exact permissions and composition. No dependency, migration,
generic repository framework or change to existing exact DTOs is required.

## Verification and delivery

Real PostgreSQL HTTP journeys cover all namespaces, concurrent equal publication,
changed content, property order, referenced versions, immutable legacy data,
numeric-version pagination, permissions, tenant isolation, suspension, rollback
and safe failures. Browser tests cover session/CSRF/origin and fresh revocation.
Contract tests validate examples and strict publication schemas. Compare all
preceding paths and components structurally.

Run the complete authoritative acceptance and OpenAPI groups. See
[ADR 0106](../../adr/0106-publish-and-discover-immutable-semantic-definitions.md)
and the [operator guide](../../operations/semantic-definitions.md).
The [master sequence](m02-m04-delivery.md) preserves the approved PR numbering.

Rollback removes additive HTTP routes, retaining definitions and older strict APIs.
Definition editing/deletion, attributed publication audit, UI editors, asset/device
registration, ontology constraints and deployment qualification remain outside this slice.


## Verification evidence

Trusted base: `07a8a793677a7e0ccc15356d7bff272c3f0eef8d` (fetched origin/main).
PostgreSQL 18.6 has all 28 existing migrations; this slice adds none.

- **Passed:** `cargo xtask verify --group acceptance --base origin/main`, all
  thirteen checks, including online SQLx validation, strict workspace Clippy,
  842 runtime tests across 61 binaries with zero skipped, four compile-fail
  doctests, and rustdoc. Evidence:
  `target/verification/acceptance-76160-1792093494615896000.json`.
- **Passed:** `.\target\debug\fleetiq-xtask.exe verify --group openapi`,
  using the same compiled authoritative runner: 64 sensor fixtures, pinned
  Redocly lint, modular ownership and exact bundle freshness. Evidence:
  `target/verification/openapi-66684-1792092796795908000.json`.
- **Passed:** Structural comparison against the trusted bundle preserves all
  29 preceding paths and 161 components exactly. Baseline.10 has 35 paths and
  184 components; its six additional paths expose nine operations.
- **Failed:** None after corrections.
- **Not run:** GitHub CI for this local, uncommitted branch.

Evidence files are ignored local execution metadata, not committed source.
ADR 0106 remains Proposed for human review. After this slice merges, continue
with original plan PR 9: asset and communication-device registration.
