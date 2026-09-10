# Architecture

FleetIQ is an asset-centric platform. Its core model supports multiple industries
and treats locomotives as one prospective configuration rather than a universal
domain assumption.

The principal boundaries are:

1. A protocol layer turns bytes from an identified communication device into
   source observations while preserving provenance.
2. A semantic layer binds source observations to typed properties of physical
   assets for an effective time interval.
3. A telemetry layer stores current and historical observations and supplies
   bounded queries.
4. An application layer implements asset management, calculations, alerts,
   commands, and user-facing workflows.
5. Adapters integrate transports, persistence, brokers, identity providers, and
   object storage without leaking them into the domain model.

The initial runtime topology and technology choices remain subject to focused
ADRs and measured prototypes. See the source redesign documents until their
accepted parts are migrated into this repository.

## Semantic asset foundation

An `Asset` is an industry-neutral physical or logical thing owned by one tenant.
Its configurable asset type supplies domain meaning; FleetIQ does not compile a
fixed list such as vehicle, locomotive, motor, or sensor into the core aggregate.

External identifiers consist of a kind, an optional issuing authority, and a
value. VIN, IMEI, serial number, and operator number can therefore coexist
without making any one identifier universal. The asset aggregate prevents exact
duplicates. PostgreSQL enforces cross-asset uniqueness within the tenant, kind,
authority, and value namespace.

Asset-type definitions are immutable and versioned. An asset references a
specific version, preventing a later configuration change from silently changing
its meaning. Each asset-type version also pins the version of every supported
property definition.

Property definitions are reusable across asset types and declare a storage-level
value kind plus an optional canonical unit for numeric values. Calculators can
therefore target semantic property IDs rather than vendor-specific fields. Unit
identifiers are configurable so adopting a standard vocabulary does not require
changing the core domain model.

Relationships are directed, typed domain facts between assets. Relationship-type
definitions are immutable and versioned. Each relationship revision records:

- its tenant and stable relationship identity;
- its recorded revision and the time FleetIQ learned that revision;
- its source and target asset roles;
- the exact relationship-type version;
- a half-open effective interval describing when it applies physically.

This distinction supports delayed information and corrections without silently
rewriting physical history. The domain interval enforces local ordering. Rules
that require the broader graph—permitted asset types, temporal overlap,
cardinality, and containment cycles—belong to the application service and
repository transaction that can inspect that graph. PostgreSQL composite foreign
keys enforce that both endpoints and the exact relationship-type version belong
to the relationship's tenant.

Communication capability is attached to an existing asset rather than modeled as
a competing device identity. A communication device can expose an ordered set of
stable endpoints. Each endpoint has an identity scoped by its device, an
extensible kind, and an operator-facing label. Examples can include onboard
measurements, a CAN interface, a serial channel, or an identified peripheral;
the core model does not prescribe a hardware topology.

Endpoints locate a source within a device. They do not contain protocol package
selection, vendor parameter selectors, or target asset/property attribution.
Device profiles and the protocol runtime interpret vendor fields, while a later
effective-time signal-binding increment will assign those source signals to
semantic asset properties.

A signal binding is a revisioned attribution from a decoded source signal to an
exact semantic property on a target asset. Its source consists of the device
asset, one endpoint beneath that device, and a signal ID emitted by a protocol
package or device profile. Its target consists of the observed asset and an exact
property-definition version. A half-open effective interval and independent
recorded revision preserve historical attribution when commissioning information
arrives late or is corrected.

The domain contract intentionally permits a device to observe its own properties
as well as properties of other assets. Repository and application boundaries
must verify tenant membership, endpoint existence, target property support, and
cross-binding policies. Unit conversion definitions and authority among multiple
sources require separate explicit contracts before telemetry normalization uses
the bindings.

## Metadata persistence

PostgreSQL migrations define the persisted metadata schema. Handwritten domain
aggregates remain independent of that schema. Application-owned repository ports
separate use cases from SQLx adapters, whose private row types reconstruct domain
objects through validated constructors.

Tenant-aware composite primary and foreign keys enforce isolation structurally.
Assets reference an asset-type definition from the same tenant and exact version.
External identifier uniqueness is scoped by tenant, identifier kind, optional
authority, and value. Aggregate writes include identifiers in one transaction.

The semantic-definition repository stores and retrieves exact property,
asset-type, and relationship-type versions. Asset-type definitions persist their
property references atomically and retain declaration order. Published
definition versions are append-only: PostgreSQL rejects updates and deletes so a
semantic change must introduce a new version.

Asset relationships have stable identities with immutable, contiguous recorded
revisions. Each revision stores directed endpoints, an exact relationship-type
version, recorded time, and a half-open effective interval. A mutable head
serializes concurrent appends; deferred constraints verify the complete revision
sequence and monotonic recorded time at commit. Snapshot queries accept separate
effective-time and recorded-time cutoffs so corrections do not rewrite what was
known previously.

Communication-device rows reuse an existing asset's tenant and identity.
Tenant-aware foreign keys prevent capability from being attached across tenant
boundaries. Endpoint identities are unique within a device and retain declaration
order during aggregate persistence and reconstruction. Device registration and
all endpoints are written atomically with one set-based PostgreSQL statement.

See [ADR 0001](../adr/0001-postgresql-sqlx-persistence.md).
