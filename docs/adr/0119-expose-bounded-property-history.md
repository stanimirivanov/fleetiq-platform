# ADR 0119: Expose bounded property history

## Status

Proposed, 27 October 2026.

Supersedes: none. Superseded by: none.

## Context

The inspector needs an asset/property timeline and an explanation of each value.
Source-scoped history already preserves immutable attribution, but metadata
permission must not implicitly disclose telemetry. Browser numbers cannot
represent the complete integer and decimal domains. Late observations and
metadata corrections preclude presenting an event-time page as a current-state
projection or a repeatable snapshot.

## Decision

History owns a bounded property query port and a service consuming typed
`AuthorizedTenant<ReadTelemetry>`. Access owns the new exact `telemetry.read`
action; existing grants are unchanged. PostgreSQL implements the port and HTTP
only validates transport, obtains fresh authorization and maps safe DTOs.

Publish history and exact interpretation routes beneath one tenant/asset/exact
property-version scope. Require an explicit half-open event-time window of at
most seven days, a page size from 1 through 100 (default 50), and an optional
exclusive full-identity key. Sort ascending by event time, message, package,
package version, output position, binding, revision and time basis. Keep every
interpretation, even when event times or source values coincide.

The page cursor includes a SHA-256 scope digest over a length-delimited tuple of
tenant, target and time window, plus the event/full-identity continuation. The
versioned opaque codec is lowercase hex JSON, bounded at 8192 ASCII bytes before
decoding. Provenance tokens encode only exact interpretation identity. Neither
is signed, secret, or authority: every request reauthenticates and reauthorizes,
and every storage predicate includes tenant scope. Clients must not construct
or edit tokens. Later incompatible codecs need a new prefix.

Read history, exact original binding and decoded output in one set-based page
query, fetching at most limit plus one. Reuse validating source/history mappers;
never query once per result or rerun semantic resolution. Add target and
binding/event indexes. Use a transaction-local five-second statement timeout;
large fan-out returns an infrastructure failure rather than an invented empty
page. The indexes improve selective access; a time window/page bound is not a
constant-time guarantee or a limit on all scanned rows.

Serialize signed integer and arbitrary-precision decimal values as tagged
strings; Boolean/text keep native JSON kinds. Times and binding revisions are
also decimal strings; output indexes and definition versions remain bounded
u32 numbers. Preserve source unit/quality, device and receipt timestamps, exact
decoder, binding revision, target definition and first-append temporal context.
Do not return accepted wire payloads or deployment/session secrets.

## Consequences

Missing or unobserved targets return empty pages without claiming metadata
existence. Exact missing or differently scoped interpretations return 404.
Knowledge cutoffs are stored evidence, not query filters. Corrections never
retarget an old row. Concurrent/late appends ahead of a cursor may appear in
later pages; appends behind it require restarting the window. No completeness,
current value, archive coverage, live state or health claim is implied.

The action vocabulary expands in OpenAPI baseline.16 and both identity tables;
clients and all serving readers must understand it before administrators grant
it. Index creation is transactional and scans deployed data under ordinary DDL
locks; schedule a maintenance window on large installations. Rollback serving
requires first removing new grants through audited commands. Preserve migration
and immutable data; do not delete or rewrite old interpretations.

## Alternatives considered

- Reuse metadata.read: rejected because metadata delegation does not authorize
  telemetry disclosure.
- Offset pagination or a latest-revision join: rejected because late insertion
  shifts offsets and corrections would erase the original explanation.
- JSON aggregation/per-row source reads: rejected in favor of ordinary checked
  SQL column typing and set-based bounded materialization.
- Denormalized asset/property copies and archival query infrastructure: deferred
  until measured volume, indexing or M06 retention requires another read model.

## Verification

CI runs bounded-query constructor tests; real PostgreSQL tests for complete
source collections, page limits/ties, late interpretation insertion, pinned
corrections, exact property versions and numeric precision; HTTP/contract tests
for exact telemetry authority, revocation, malformed/scope-mismatched cursors,
absence, safe infrastructure failure and no-store. Strict workspace, migration,
security, SQLx freshness and OpenAPI checks remain required. These gates are
not executed locally for this task at the owner's request; no acceptance pass
is inferred from code generation or review.
