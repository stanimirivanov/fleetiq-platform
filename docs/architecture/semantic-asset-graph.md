# Semantic asset graph

## TL;DR

FleetIQ models customer equipment as typed assets connected by revisioned,
effective-dated relationships. Communication devices expose endpoints; temporal
signal bindings attribute decoded source signals to exact properties on any
tenant-owned asset. Industry-specific concepts remain configuration rather than
core assumptions.

## Definitions and assets

Asset-type, property, and relationship-type definitions are immutable and
versioned. An asset pins one asset-type version, and that version pins each
supported property-definition version. Configuration changes therefore cannot
silently reinterpret existing assets or history.

An asset has a FleetIQ identity. External identifiers add typed namespaces such
as IMEI, serial number, fleet number, or a future rail identifier. Their kind,
optional issuing authority, and value are unique within a tenant-defined
namespace. No identifier is universal.

## Relationships and time

Relationships are directed, typed edges between assets. Each revision records
the stable relationship identity, source and target assets, exact relationship
type version, recorded revision and time, and half-open effective interval.

Effective time says when a fact applies in the physical world. Recorded time
says when FleetIQ learned the revision. Keeping both supports late data and
corrections without destructive history edits. Graph-wide rules such as role
compatibility, cardinality, overlapping intervals, and containment cycles
belong in an application operation with repository access.

The [revision command API](../operations/metadata-revisions.md) appends complete
facts under exact tenant management authority. Explicit successive revisions
correct history; equal retries of an older attributed fact preserve its original
actor/time even after later corrections. Existing strict writes remain unattributed.

## Devices and endpoints

A communication device is an asset with an ordered set of stable endpoints.
An endpoint identifies a source boundary such as an onboard measurement group,
CAN channel, serial channel, or peripheral. Endpoint kinds remain extensible.

Endpoints do not select protocol packages and do not assign business meaning.
Package activation interprets vendor traffic. Signal bindings perform semantic
attribution after decoding.

Creation through the [registration API](../operations/metadata-registration.md)
commits each aggregate and its complete children with immutable first-actor/time
evidence. Exact retries compare all creation content; external identifiers are a
set and endpoints retain order. Legacy strict inserts have unknown attribution.
A device remains communication capability on its existing asset identity.

## Signal bindings

A binding connects:

```text
(device asset, endpoint, source signal)
    -> (observed asset, exact property-definition version)
```

Bindings are revisioned and effective-dated. Resolution selects the latest
revision known at a recorded-time cutoff and then evaluates its effective
interval at the observation's chosen event time. A device can observe itself or
another asset, including a component attached through the graph.

Successful resolution preserves binding provenance and produces an asset
property change candidate. Missing, ambiguous, or type-incompatible
configuration remains an explicit unresolved outcome so operators can diagnose
and retry after configuration changes.

The protected [source-binding snapshot](../operations/signal-binding-snapshot.md)
provides a bounded diagnostic read of recorded attributions for one decoded
source. The [asset-binding snapshot](../operations/asset-binding-snapshot.md)
provides the reverse view for one target asset. Neither executes semantic
resolution or materializes property state.

## Persistence invariants

PostgreSQL composite keys prevent cross-tenant references. Foreign keys pin
exact definition versions and device endpoints. Append operations serialize on
stable identities where revision order matters, then read dependent current
state in a fresh `READ COMMITTED` statement when PostgreSQL snapshot semantics
require it.

The database enforces durable identity and reference invariants. Application
services enforce rules that require graph traversal, temporal comparison across
aggregates, or business authorization.

See [ADR 0003](../adr/0003-explicit-semantic-resolution.md) and the metadata
migrations for the detailed accepted contracts.

## Bounded graph exploration

The [graph read API](../operations/semantic-graph.md) uses metadata-owned
breadth-first policy and a PostgreSQL read-only repeatable snapshot. It expands
relationship and signal-binding adjacency in both directions, retaining typed
directed facts. Validated depth/asset/link limits, unique vertices, closed
endpoints and explicit truncation prevent unbounded traversal or implied
inventory completeness. Latest-known selection precedes endpoint/effective
filters. No graph database, telemetry read or projection is involved. See
[ADR 0120](../adr/0120-explore-bounded-semantic-graphs.md).

## Current telemetry projection

The [current-property projector](../operations/current-property-state.md) materializes
immutable interpretations independently of graph traversal. Exact property
snapshots require telemetry.read and preserve quality/provenance; metadata.read
does not reveal telemetry. Event-time arbitration, atomic revisions and rebuild
generations belong to projection. The [live transport](../operations/property-live-updates.md)
synchronizes complete exact-property state without reinterpreting the graph.
