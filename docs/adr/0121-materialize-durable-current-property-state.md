# ADR 0121: Materialize durable current property state

## Status

Proposed, 27 October 2026.

Supersedes: none. Superseded by: none.

## Context

Immutable history now preserves every interpretation, but an inspector needs
current evidence without reading a growing event window or choosing its own
arrival-order policy. A sequence watermark is unsafe: an earlier allocated ID
can commit after a later batch. Materialization must be rebuildable, expose
committed revisions, retain precision and prevent mixed-snapshot reads.

## Decision

Add `fleetiq-projection` as an application capability owning precedence,
freshness, typed telemetry-read service and atomic maintenance/read ports.
`fleetiq-live` stays a dependency-free contract crate. PostgreSQL implements
set-based atomic maintenance; the server owns process and HTTP composition.
There are no new external dependencies or policy exceptions.

### Current-value policy v1

Each exact tenant/asset/property/version has at most one winner. Choose the
greatest tuple: event timestamp, message ID, protocol package ID/version,
output index, binding ID, numeric binding revision and event-time basis.
Strings use UTF-8 byte order (`COLLATE "C"`); device-recorded sorts before
received-at. The application comparator is the reference policy; adapter
regressions compare its result with database arbitration.

Event time dominates arrival. Identity tie-breaking is deterministic arbitration,
not semantic trust in lexical identifiers or different device clocks. Preserve
invalid/uncertain quality on the latest value; there is no implicit last-good
fallback or unit conversion. Source timestamps, exact binding revision and
first-resolution context remain immutable. A binding correction adds a new
interpretation; it does not retract an old target's history or re-resolve past
facts. Future event times remain visible and explicitly classified as future.

### Atomic maintenance

Migration 0037 adds tenant heads, pending interpretation references and current
winner pointers. An AFTER INSERT history trigger creates pending work in the
same transaction. Existing history is backfilled transactionally. Exact retries
that do not append history create no new work. No value/payload copy is stored.

A projector locks active lifecycle and tenant head, selecting least-recently
served work with SKIP LOCKED. At most 100 pending facts become deterministic
per-target candidates, winner pointers and a processed-count checkpoint in one
transaction. The revision advances once per nonempty committed batch, including
batches of older facts that leave values unchanged. No work means no revision.
Queue IDs are scheduling hints only. Cancellation/error rolls back all changes;
an ambiguous successful commit is safe to retry without double counting.
Suspended tenants retain pending work until active again.

Atomic rebuild requires REPEATABLE READ (also enforced by the SQL function),
locks the same tenant head, reconstructs every target from snapshot-visible
history, consumes only visible pending rows and increments generation and
revision. The processed count becomes the visible history count. Later commits
retain their queue rows. A conflicting writer can produce a serialization
failure: retry the unchanged maintenance request after inspecting the cause.
Rebuild does not reset continuity revisions or delete immutable evidence.

### Snapshot and resource contract

Publish an additive exact-property GET ending in `/versions/{version}/current`.
Authenticate and obtain fresh `telemetry.read` before scope validation/discovery.
The exact property must be installed on the asset: unknown scope returns 404;
known scope without a materialized value returns an empty complete projection.
One repeatable-read view loads metadata existence, head/pending state and at most
one winner joined to the existing immutable history/source mapper. No per-item
queries or independently timed values/revisions are permitted.

Return generation, revision and processed count as exact decimal strings,
evaluation time, a 30,000-ms age threshold, pending flag and zero/one item using
the existing history value/provenance schema. Fresh covers age 0..30000 ms,
stale covers greater ages, and future covers timestamps after evaluation. This
is display coverage, not health. Pending refers only to committed interpretation
work for this tenant; unresolved semantic work and upstream queues are separate.

Maintenance statements have a five-second deadline, snapshot statements two
seconds and whole snapshot request/rebuild turns eight seconds. Snapshot
transactions have an eight-second idle limit. These limit occupancy, not total
data scanned or rebuild capacity. A large rebuild can fail safely and remains
an operator action requiring later scale qualification. The worker has two pool
connections, one in-flight turn and a 250-ms interruptible polling delay.

## Consequences

Current data and progress are consistent across restarts, duplicates and
out-of-order commits. Rebuilds replace state atomically; generations identify
baseline replacement for the later live transport. Separate snapshots may see
new revisions; no stream retention, delta replay or global ordering is promised.
Revision comparison is meaningful only within the same tenant projection.

The enqueue trigger adds privileges to the semantic writer's reviewed database
role. Grant them before resuming writers; missing permissions fail the entire
history append. Functions use invoker privileges, not SECURITY DEFINER. Migration
backfill scans existing history and holds schema locks; large installations
must quiesce semantic writers and plan the migration window. D06 and full-profile
installation remain deferred; the opt-in Compose worker is local development.

## Alternatives considered

- Query full history on every display: repeats growing work and obscures revisions.
- Use sequence MAX as a checkpoint: loses late commits behind the watermark.
- Put services/repositories in live: violates the established contract boundary.
- A separate broker or graph database: unnecessary for this transactional slice.
- Automatic retraction after metadata correction: needs a separately defined
  interpretation invalidation contract; silently deleting old evidence is wrong.

## Verification

Pure tests cover event-time/tie ordering and freshness boundaries/overflow.
Real PostgreSQL regressions cover predecessor migration backfill, queue gaps,
rollback, concurrent bounded workers, exact retry, restart/drain, rebuild parity,
a rebuild race, isolation enforcement, corrupt lineage, corrections, tenant
scope, suspension, exact decimals and latest invalid quality. HTTP tests cover
schema/example parity, absence, stale evidence, integer precision, authorization,
revocation, suspension and safe infrastructure failure. Focused compile/lint,
structural checks and OpenAPI run locally; heavy database/workspace/image gates
remain with CI at the owner's request.
