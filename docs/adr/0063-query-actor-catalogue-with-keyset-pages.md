# ADR 0063: Query the actor catalogue with keyset pages

## Status

Accepted, 28 September 2026.

Supersedes: none. Superseded by: none.

## Context

FleetIQ can provision, activate, deactivate, and audit actors, but an identity
administrator has no supported way to discover stable actor identities or their
current operational state. Requiring an administrator to infer current actors
from provisioning or lifecycle history would be incomplete, while direct
PostgreSQL access would couple operational tooling to private persistence.

The persisted actor model currently contains a stable opaque identity and an
active flag. External identity subjects are authentication data with separate
administration and audit boundaries; a current-state actor catalogue does not
need to disclose them.

## Decision

Add an `ActorCatalog` application service that consumes authorization evidence
for exactly `ManageIdentity`. This is the existing authority for actor and
external-identity administration. Other platform actions and all tenant-scoped
actions cannot enumerate actors.

Each entry exposes a validated `ActorId` and an explicit
`ActorLifecycleState` of `Active` or `Deactivated`. The application contract
does not expose the persisted Boolean representation or external identities.

Queries require a validated page size from 1 through 100 and an optional
exclusive cursor. Results use ascending stable-identity order:

```text
actor_id ASC
```

The cursor contains the final actor identity returned by the preceding page.
The PostgreSQL repository selects `actor_id > cursor`, fetches one row beyond
the requested page size, removes that sentinel, and returns a continuation only
when later identities remain. The existing actor primary-key index supports
this order, so no schema migration is required.

## Consequences

Authorized identity administrators can discover every current actor and
distinguish active from deactivated identities without reading private database
tables or exposing authentication subjects. Actor identities remain opaque and
provider-neutral.

The cursor is a traversal position rather than a database snapshot. An actor
inserted after traversal begins appears according to its identity: it can be
seen on a later page when it sorts after the cursor, while an insertion before
the cursor is outside that continuing traversal.

The initial catalogue is platform-wide and accepts no caller-selected filter.
It exposes current state only; immutable provisioning, external-identity, and
lifecycle evidence remain available through their separate audit boundaries.

This slice adds no HTTP representation, cursor encoding, display profile,
search, filtering, external-identity expansion, deletion, or tenant membership
view.

## Alternatives considered

Reading actors from provisioning or lifecycle audit history was rejected
because bootstrap actors and unchanged actors may not appear in those specific
histories, and audit evidence is not a current-state catalogue.

Offset pagination was rejected because concurrent inserts can shift positions
and duplicate or omit entries between requests.

Returning the persisted `is_active` Boolean was rejected because callers should
consume an explicit lifecycle state rather than depend on storage encoding.

Including external identity subjects was rejected because catalogue discovery
does not require authentication identifiers and would expand disclosure beyond
the current-state use case.

Adding a new platform read action was deferred because the current consumer is
the identity administrator. A separately delegated discovery role can introduce
an exact action when a concrete least-privilege workflow requires it.
