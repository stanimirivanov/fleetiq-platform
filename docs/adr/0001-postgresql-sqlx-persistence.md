# ADR 0001: PostgreSQL migrations and SQLx persistence adapters

- Status: accepted
- Date: 2026-09-09

## Context

FleetIQ needs a transactional metadata store for tenants, versioned semantic
definitions, assets, identifiers, relationships, bindings, and workflows. The
domain model contains invariants that do not map directly to database rows, while
tenant-wide uniqueness and referential integrity require database enforcement.

The persistence approach must support asynchronous Rust applications, explicit
PostgreSQL features, compile-time checking of static SQL, reviewable migrations,
and separation between domain and storage representations.

## Decision

PostgreSQL migration files are the source of truth for the persisted metadata
schema. SQLx is the PostgreSQL access library.

Domain aggregates remain handwritten in `fleetiq-domain`. Application-owned
repository ports live in `fleetiq-application`. Private row types, SQL queries,
transactions, and row-to-domain mapping live in `fleetiq-postgres`.

Static production queries use SQLx's checked query macros. CI applies migrations
to PostgreSQL before compiling them. Integration tests run against isolated real
PostgreSQL databases through SQLx's test support.

The initial schema makes semantic definitions tenant-owned. A shared managed
catalogue, if required, will be introduced through a later explicit ownership and
override design rather than nullable tenant ownership.

Internal and semantic identifiers remain validated text in the initial schema,
matching the existing domain types. Identifier generation policy is an
application concern and may introduce a more specific internal format before a
public compatibility commitment.

## Consequences

- Database rows do not become domain or API models.
- Mapping code is explicit and reconstructs values through domain validation.
- PostgreSQL constraints duplicate important local validation and enforce
  invariants involving multiple rows or concurrent transactions.
- Builds using checked queries require a migrated development database or
  prepared SQLx offline metadata. CI uses a migrated database initially; offline
  metadata can be added when the query set stabilizes.
- PostgreSQL-specific schema and queries are intentional. Portability is provided
  by repository boundaries rather than restricting SQL to a lowest common
  denominator.
- Definition sharing across tenants initially requires deliberate replication.

## Alternatives considered

### SeaORM-generated entities

Schema generation is convenient for CRUD operations, but generated persistence
entities expose table shape and ORM behavior. Using them as domain aggregates
would couple business rules to storage representation.

### Diesel

Diesel provides strong schema and query-builder typing. FleetIQ expects temporal,
recursive, bulk, and operational PostgreSQL queries where explicit SQL and SQLx's
asynchronous model are a better fit.

### SQL schema as the only model

Database constraints remain essential, but a schema alone cannot express all
in-memory behavior, typed state transitions, or boundaries shared with non-SQL
telemetry stores.
