# ADR 0059: Query actor lifecycle audit with keyset pages

## Status

Accepted, 28 September 2026.

Supersedes: none. Superseded by: none.

## Context

[ADR 0058](0058-administer-actor-lifecycle-with-atomic-audit.md) established
immutable activation and deactivation evidence, but the application has no
supported way to read that history. Identity administrators need a bounded,
deterministic view of who changed actor availability and whether each command
changed current state.

Recorded time alone is not a stable pagination position because commands can
share one timestamp. Offset pagination can also shift when newer records are
appended.

## Decision

Add an `ActorLifecycleAudit` application service that consumes authorization
evidence for exactly `ManageIdentity`. Other platform actions and all tenant
actions cannot read global actor lifecycle history.

The application query requires a validated page size from 1 through 100 and an
optional cursor. Results are newest first by:

```text
(recorded_at_ms DESC, change_id DESC)
```

The cursor contains both fields from the last returned entry. The repository
uses strict tuple comparison `(recorded_at_ms, change_id) < (time, id)`, fetches
one row beyond the requested page size, removes that sentinel, and returns a
continuation only when older history remains.

Each entry reconstructs the change ID, administrator and target actors,
activation or deactivation kind, trusted recorded time, and state-change effect
through validated application and domain types. Unknown durable kinds and
invalid identifiers are corrupt data.

Migration 0022 adds a descending index over `(recorded_at_ms, change_id)` for
the exact query order. Transactional index creation can block writes on a
populated audit table and must be measured before a large deployment.

## Consequences

Authorized identity administrators can retrieve complete immutable lifecycle
history in bounded, repeatable pages. Equal timestamps have deterministic
order, and newer inserts do not shift an older continuation boundary.

The cursor is a traversal position rather than a database snapshot. Audit rows
cannot be updated or deleted, but a privileged operational insertion carrying
an older recorded time may appear on a later page according to its key.

The audit is platform-wide and accepts no caller-selected tenant or actor
scope. This initial complete history supports investigation without committing
to filter behavior or additional indexes before operator needs are measured.

This slice adds no HTTP representation, cursor encoding, filtering, search,
export, retention policy, or administration interface.

## Alternatives considered

Offset pagination was rejected because concurrent inserts can shift positions
and duplicate or omit entries between requests.

Using recorded time alone was rejected because commands may share one
millisecond and require a total order.

Returning unvalidated strings was rejected because persistence adapters must
not let corrupt durable values cross into application behavior.

Adding actor or operation filters now was rejected because the first need is a
complete ordered history. Filter semantics and indexes should follow measured
operator requirements.
