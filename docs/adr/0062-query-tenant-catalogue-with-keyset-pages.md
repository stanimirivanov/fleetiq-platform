# ADR 0062: Query the tenant catalogue with keyset pages

## Status

Accepted, 28 September 2026.

Supersedes: none. Superseded by: none.

## Context

FleetIQ can provision, activate, suspend, and audit tenants, but a platform
operator has no supported way to discover the stable tenant identities or their
current operational state. Requiring an operator to learn identifiers from
audit history or inspect PostgreSQL would make lifecycle administration
incomplete and couple operational tooling to private persistence.

The persisted tenant model currently contains a stable opaque identity and an
active flag. It does not yet define a display name, customer profile, billing
state, or other descriptive catalogue fields.

## Decision

Add a `TenantCatalog` application service that consumes authorization evidence
for exactly `ManageTenantLifecycle`. This matches the authority already needed
to change tenant availability and does not introduce a broader read capability.
Other platform actions and all tenant-scoped actions cannot enumerate tenants.

Each entry exposes a validated `TenantId` and an explicit
`TenantLifecycleState` of `Active` or `Suspended`. The application contract does
not expose the persisted Boolean representation.

Queries require a validated page size from 1 through 100 and an optional
exclusive cursor. Results use ascending stable-identity order:

```text
tenant_id ASC
```

The cursor contains the final tenant identity returned by the preceding page.
The PostgreSQL repository selects `tenant_id > cursor`, fetches one row beyond
the requested page size, removes that sentinel, and returns a continuation only
when later identities remain. The existing tenant primary-key index supports
this order, so no schema migration is required.

## Consequences

Authorized lifecycle operators can discover every current tenant and distinguish
active from suspended boundaries without reading private database tables.
Tenant identities remain opaque and industry-neutral.

The cursor is a traversal position rather than a database snapshot. A tenant
inserted after traversal begins appears according to its identity: it can be
seen on a later page when it sorts after the cursor, while an insertion before
the cursor is outside that continuing traversal.

The initial catalogue is platform-wide and accepts no caller-selected filter.
It exposes current state only; immutable lifecycle history remains available
through the separate tenant lifecycle audit service.

This slice adds no HTTP representation, cursor encoding, display metadata,
search, filtering, deletion, billing state, or tenant-owned catalogue view.

## Alternatives considered

Reading tenant identities from lifecycle audit history was rejected because an
unchanged tenant may have no lifecycle record and audit evidence is not a
current-state catalogue.

Offset pagination was rejected because concurrent inserts can shift positions
and duplicate or omit entries between requests.

Returning the persisted `is_active` Boolean was rejected because callers should
consume an explicit lifecycle state rather than depend on storage encoding.

Adding a new platform read action was deferred because the current consumer is
the lifecycle operator. A separately delegated discovery role can introduce an
exact action when a concrete least-privilege workflow requires it.
