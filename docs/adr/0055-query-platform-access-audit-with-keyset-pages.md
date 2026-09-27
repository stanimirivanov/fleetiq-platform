# ADR 0055: Query platform access audit with keyset pages

## Status

Accepted, 27 September 2026.

Supersedes: none. Superseded by: none.

## Context

[ADR 0054](0054-administer-platform-grants-with-atomic-audit.md) established
immutable platform grant-change evidence, but the application has no supported
way to read that history. Operators need a bounded and deterministic audit
view without receiving unbounded rows or bypassing the same exact platform
authority required to change grants.

Recorded time alone is not a stable pagination position because several
commands may share one trusted host timestamp. Offset pagination can duplicate
or skip entries as newer commands are appended between requests. Durable text
values must also be reconstructed into validated domain types before they cross
the PostgreSQL adapter boundary.

## Decision

Add a `PlatformAccessAudit` application service that consumes authorization
evidence for exactly `ManagePlatformAccess`. Evidence for identity management,
tenant provisioning, or any tenant action cannot read the global platform
access history.

The application query requires a validated page size from 1 through 100 and an
optional continuation cursor. Results are newest first by the complete order:

```text
(recorded_at_ms DESC, change_id DESC)
```

The cursor contains both fields from the last returned entry. The repository
uses the strict tuple comparison `(recorded_at_ms, change_id) < (time, id)` to
select older rows. It fetches one row beyond the requested size, removes that
sentinel, and returns a continuation only when more history exists.

Each entry reconstructs the platform change ID, administrator and target actor
IDs, exact platform action, grant or revoke kind, trusted recorded time, and
state-change effect into application and domain types. Unknown persisted action
or operation values and invalid identifiers are reported as corrupt data rather
than exposed as unchecked strings.

Migration 0018 adds a descending index over `(recorded_at_ms, change_id)` for
the exact repository order. The migration documents that transactional index
creation can block writes on a populated table and must be measured before a
large production deployment.

## Consequences

Authorized platform administrators can retrieve the complete immutable audit
history in bounded, repeatable pages. Equal timestamps remain deterministic,
and appending newer commands between page requests does not shift the older
continuation boundary.

The cursor describes traversal position rather than snapshot isolation. A row
cannot be updated or deleted, but an operational database intervention could
insert an older recorded time after a page was read. Such a row may appear on a
later page according to its durable ordering key.

The audit is platform-wide, so the repository accepts no tenant or caller-
selected scope. Authorization evidence controls access but is not passed to
SQL as an audit filter.

This slice adds no HTTP representation, cursor encoding, filtering, search,
export, retention rule, or user interface. Transport adapters must treat cursor
fields as opaque continuation state and preserve the application page bound.

## Alternatives considered

Offset pagination was rejected because concurrent inserts can shift offsets
and produce duplicate or missing rows across pages.

Using recorded time alone was rejected because valid commands can share the
same millisecond and would not have a total order.

Allowing any platform action to read the audit was rejected because access
history exposes security-sensitive actor and authority changes. The exact
grant-administration authority is required.

Returning persisted strings directly was rejected because adapter boundaries
must validate durable data before constructing application values.

Adding filters in the first query was rejected because the immediate need is a
complete deterministic history. Filter semantics and supporting indexes should
follow concrete operator and retention requirements.
