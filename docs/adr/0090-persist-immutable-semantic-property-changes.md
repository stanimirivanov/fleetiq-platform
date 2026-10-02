# ADR 0090: Persist immutable semantic property changes

## Status

Accepted, 2 October 2026.

Supersedes: none. Superseded by: none.

## Context

ADR 0006 defines a deterministic identity for a successfully bound property
change, but the result has no durable home. Decoded observations and binding
revisions are already immutable. A later worker needs an idempotent handoff to
history before live projection or analytics can consume semantic facts. Storing
only the current asset value would discard correction and replay evidence.

## Decision

Introduce application-owned `fleetiq-history::AssetPropertyChangeLog` and an
append-only PostgreSQL `history.asset_property_changes` table. The primary key
is the complete ADR 0006 identity: tenant, decoded observation identity, exact
binding revision, and explicit event-time basis. Each row stores the selected
event time and the first successful bitemporal resolution context. Immutable
decoded and binding rows remain authoritative for value, quality, source, and
target; history does not copy those fields into independently mutable columns.

Composite foreign keys and an insert trigger enforce tenant-safe references,
source and binding alignment, exact property kind and unit, latest-known
binding revision at the recorded cutoff, effective interval, and selected
event time. The PostgreSQL adapter additionally compares all caller-supplied
observation content before appending. An exact retry returns `AlreadyPresent`;
same-identity content disagreement or missing durable facts fails. A later
knowledge cutoff that selects the same exact interpretation is an exact retry;
the first persisted context remains evidence of the initial append.

The migration is additive, contains no backfill, and does not alter existing
ingestion or metadata rows. Row-level update and delete are rejected. A future
retention plan must govern partition or archival lifecycle explicitly.

## Consequences

Semantic interpretations become durable independently of live-state eligibility
or analytics policy. A corrected binding revision and a different event-time
basis create distinct history identities for the same decoded observation.
PostgreSQL foreign keys prevent an interpretation from outliving its source
facts; direct deletion of parents is already outside their append-only model.

The append currently reads durable facts before inserting one row, and the
trigger checks lineage again at insertion to close a concurrent-correction
gap. This is a small append boundary, not a batch worker or a historical query
API. Before a high-volume worker uses it, append batching and query latency
need measured design work. It does not choose source authority, quality
acceptance, clock trust,
latest-state replacement, retention, or cross-binding ambiguity policy. Those
remain downstream decisions. Rollback requires removing this new capability
and its table through a forward migration; earlier schemas are unchanged.

## Alternatives considered

Putting the write port in `fleetiq-semantics` would make read-only attribution
policy own persistence effects. Reusing `fleetiq-ingestion` would mix protocol
facts with semantic history. Copying the decoded value and target into the
history row would make retries compare a second copy of immutable source facts
and increase storage; a future measured query requirement may justify a derived
projection instead.
