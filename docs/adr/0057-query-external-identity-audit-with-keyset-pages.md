# ADR 0057: Query external-identity audit with keyset pages

## Status

Accepted, 27 September 2026.

Supersedes: none. Superseded by: none.

## Context

[ADR 0056](0056-administer-external-identities-with-atomic-audit.md) established
immutable attach and detach evidence, but the application has no supported way
to read that history. Identity administrators need a bounded, deterministic
audit view without bypassing the exact platform authority required to change
login mappings.

The audit contains opaque external subjects. Authorized callers need the exact
value for investigation, while routine debug output must not copy that value
into logs. Recorded time alone is not a stable pagination position because
commands can share one timestamp, and offset pagination can shift when newer
records are appended.

## Decision

Add an `ExternalIdentityAudit` application service that consumes authorization
evidence for exactly `ManageIdentity`. Platform grant administration, tenant
provisioning, and tenant actions cannot read global identity history.

The application query requires a validated page size from 1 through 100 and an
optional cursor. Results are newest first by:

```text
(recorded_at_ms DESC, change_id DESC)
```

The cursor contains both fields from the last returned entry. The repository
uses strict tuple comparison `(recorded_at_ms, change_id) < (time, id)`, fetches
one row beyond the page size, removes that sentinel, and returns a continuation
only when older history remains.

Each entry reconstructs the change ID, administrator and target actors, exact
authority and opaque subject, attach or detach kind, trusted recorded time, and
state-change effect through validated application and domain types. Unknown
durable kinds and invalid identifiers are corrupt data. The entry's explicit
accessor returns the exact identity to an authorized caller, while its `Debug`
implementation replaces the subject with `<redacted>`.

Migration 0020 adds a descending index over `(recorded_at_ms, change_id)` for
the exact query order. Transactional index creation can block writes on a
populated audit table and must be measured before a large deployment.

## Consequences

Authorized identity administrators can retrieve complete immutable identity
history in bounded, repeatable pages. Equal timestamps have deterministic
order, and newer inserts do not shift an older continuation boundary.

The cursor is a traversal position rather than a database snapshot. Audit rows
cannot be updated or deleted, but a privileged operational insertion carrying
an older recorded time may appear on a later page according to its key.

The audit is platform-wide and accepts no caller-selected tenant scope. Its
opaque subjects remain available to the authorized use case but are redacted
from routine debug formatting. Transport adapters must apply equivalent
handling to logs and errors.

This slice adds no HTTP representation, cursor encoding, filtering, search,
export, retention policy, subject masking in an operator response, or user
interface.

## Alternatives considered

Offset pagination was rejected because concurrent inserts can shift positions
and duplicate or omit entries between requests.

Using recorded time alone was rejected because commands may share one
millisecond and require a total order.

Returning unvalidated strings was rejected because persistence adapters must
not let corrupt durable values cross into application behavior.

Deriving `Debug` for the complete entry was rejected because it would include
the opaque external subject in routine diagnostics.

Adding filters now was rejected because the first need is a complete ordered
history. Filter semantics and indexes should follow measured operator needs.
