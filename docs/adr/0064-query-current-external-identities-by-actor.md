# ADR 0064: Query current external identities by actor

## Status

Accepted, 28 September 2026.

Supersedes: none. Superseded by: none.

## Context

FleetIQ can provision actors, attach and detach external identities, resolve an
authenticated identity, and query immutable identity-change history. The actor
catalogue deliberately omits authentication subjects, however, so an identity
administrator cannot discover which login identities are currently attached to
a selected actor. Inferring current mappings from audit history is incomplete
and error-prone, while direct PostgreSQL access bypasses the application
authorization boundary.

External subjects are opaque, case-sensitive security identifiers. Authorized
identity administration needs their exact values, but routine diagnostics must
not copy them into logs. One actor may have several mappings, so the query must
remain bounded even if recovery and provider-migration identities accumulate.

## Decision

Add an `ExternalIdentityCatalog` application service that consumes
authorization evidence for exactly `ManageIdentity`. The caller selects one
validated stable actor ID; the repository returns only current mappings for
that actor. Other platform actions and tenant-scoped actions cannot use this
boundary.

Each entry returns the exact validated authority and opaque subject to the
authorized caller. Entry and cursor `Debug` implementations replace the
subject with `<redacted>`.

Queries require a validated page size from 1 through 100 and an optional
exclusive cursor. Results use ascending order:

```text
(authority_id ASC, subject ASC)
```

The cursor contains both values from the final entry returned by the preceding
page. PostgreSQL selects the same actor and identities strictly greater than
the cursor tuple, fetches one additional row, removes that sentinel, and emits
a continuation only when later mappings remain.

Migration 0025 replaces the actor-only mapping index with a composite index on
`(actor_id, authority_id, subject)`. Its leading actor column retains the
foreign-key and lifecycle access path while the remaining columns support the
exact catalogue filter, order, and cursor. Transactional index creation can
block writes on a populated mapping table and must be measured before a large
deployment.

## Consequences

Authorized identity administrators can inspect the current login identities
for a selected actor without enumerating every platform principal or reading
private tables. Current mapping state remains distinct from immutable identity
change evidence.

An existing actor without mappings returns an empty terminal page. A missing
actor currently has the same result; consumers that need to distinguish those
cases use the actor catalogue before requesting mappings. This keeps the query
to one indexed round trip and avoids introducing a second existence contract.

The cursor is a traversal position rather than a database snapshot. A mapping
attached after traversal begins appears according to its ordered position; one
detached before a later page is naturally absent. Exact subjects are returned
only through the authorized result and still require transport-level log and
error discipline.

This slice adds no HTTP representation, cursor encoding, authority metadata,
subject masking in an operator response, reverse lookup by external identity,
search, filtering, export, or user interface.

## Alternatives considered

Including external identities in every actor catalogue entry was rejected
because it would create unbounded nested results and disclose authentication
subjects during routine actor discovery.

A platform-wide mapping catalogue was rejected because the immediate workflow
starts from a selected actor and does not require bulk principal disclosure.

Reading current state from immutable audit history was rejected because
bootstrap and provisioning mappings may have no administration change record,
and reconstructing attach and detach effects duplicates the current-state
table's purpose.

Offset pagination was rejected because concurrent mapping changes can shift
positions and repeat or skip records.

Keeping the actor-only index and sorting each actor's mappings was rejected
because it does not provide an index-backed cursor and order as mapping counts
grow.
