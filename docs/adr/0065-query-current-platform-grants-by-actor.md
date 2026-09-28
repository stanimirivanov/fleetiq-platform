# ADR 0065: Query current platform grants by actor

## Status

Accepted, 28 September 2026.

Supersedes: none. Superseded by: none.

## Context

FleetIQ can grant and revoke exact platform actions, evaluate current platform
authorization, and query immutable grant-change history. A platform access
administrator still cannot retrieve the grants currently held by a selected
actor. Inferring state from audit history is incomplete because bootstrap
grants predate administration audit and repeated ensure-state commands may not
change the current table.

The platform action vocabulary is deliberately closed and small, but current
state still belongs behind the application authorization boundary. Direct
database inspection would couple administration tooling to private storage and
bypass the exact authority used for platform grant changes.

## Decision

Add a `PlatformGrantCatalog` application service that consumes authorization
evidence for exactly `ManagePlatformAccess`. The caller selects one validated
stable actor ID; the repository returns only current exact grants for that
actor. Identity management, tenant provisioning, tenant lifecycle management,
and tenant-scoped actions cannot use this boundary.

Each entry contains one validated `PlatformAction`. Queries require a page size
from 1 through 100 and accept an optional exclusive cursor. Results follow the
ascending persisted action-name order:

```text
action ASC
```

The cursor carries the final action returned by the preceding page. The
PostgreSQL adapter converts it to the canonical persisted action name, selects
`action > cursor`, fetches one additional row, removes that sentinel, and emits
a continuation only when later grants remain. Unknown durable action text is
reported as corrupt data rather than crossing the adapter boundary.

The existing primary key on `(actor_id, action)` supports the exact actor
filter, cursor, and ordering. No migration or additional index is required.

## Consequences

Authorized platform access administrators can inspect an actor's present
platform authority without reading private tables or replaying audit evidence.
Current grants remain separate from immutable change history.

An actor without platform grants returns an empty terminal page. A missing
actor has the same result; consumers that need to distinguish those cases use
the actor catalogue first. This preserves one indexed repository round trip.

The cursor is a traversal position rather than a database snapshot. A grant
added or removed between page requests appears or disappears according to its
ordered position. The action set is currently smaller than the maximum page,
but the bounded contract remains consistent with other administration queries
and accommodates future explicit platform actions.

This slice adds no HTTP representation, cursor encoding, actor expansion,
effective-role model, search, filtering, bulk export, mutation, or user
interface.

## Alternatives considered

Embedding platform grants in every actor catalogue entry was rejected because
it would introduce nested collections, couple routine identity discovery to
access administration, and broaden disclosure to identity administrators.

A platform-wide list of every actor-and-action pair was rejected because the
immediate administration workflow begins with a selected actor and does not
require bulk authority enumeration.

Replaying immutable change history was rejected because bootstrap grants have
no platform access change row and audit records capture requested effects rather
than serving as the current authorization source of truth.

Returning an unbounded vector was rejected even though the current vocabulary
is small. A uniform bounded contract prevents a future action expansion from
silently changing resource behavior.

Adding a new index was rejected because the existing composite primary key has
the exact required leading filter and ordered cursor columns.
