# ADR 0030: Store accepted messages as immutable ingestion facts

- Status: accepted
- Date: 2026-09-14

## Context

The application now owns an `AcceptedMessageLog` port with an idempotent append
contract. A PostgreSQL adapter will need durable storage that preserves exact
wire bytes and trusted provenance, prevents cross-tenant device references, and
supports atomic resolution of first append, exact retry, and identity collision.

Existing tables live in the `metadata` schema and describe configuration whose
lifecycle differs from high-volume ingestion evidence. Accepted messages are
append-only facts used for replay and acknowledgement safety. Their retention,
backup, access, and future partitioning will evolve independently of metadata.

The database cannot determine whether an existing row is an exact retry merely
from a primary-key conflict. The adapter must compare all immutable content
under concurrency. The schema must ensure that the row cannot be changed between
that comparison and later replay.

## Decision

Add the `ingestion` schema and its first table,
`ingestion.accepted_messages`. Each row stores:

- tenant-scoped accepted-message identity;
- admitted communication-device asset identity;
- protocol-package identity and immutable release;
- signed Unix-millisecond receipt time; and
- exact opaque frame bytes in `BYTEA`.

Use `(tenant_id, message_id)` as the primary key. Message identity is scoped by
tenant, matching other tenant-owned persistence paths and allowing future tables
to reference accepted input without losing structural tenant isolation.

Reference `(tenant_id, device_asset_id)` to
`metadata.communication_devices`. A message therefore cannot name an asset from
another tenant or an asset that lacks communication-device capability.

Validate message and package identifiers through the existing
`metadata.is_valid_identifier` function. Package fields remain opaque text until
a package catalogue and activation model provide a version-pinned foreign key.

Store receipt time as `BIGINT` signed Unix milliseconds to match
`UnixTimeMillis`. Store payload as non-null `BYTEA` and permit zero bytes because
empty-frame meaning belongs to the protocol package. Do not add a database-wide
payload maximum; host framing applies the configured package or tenant limit
before append.

Reject row-level `UPDATE` and `DELETE` with a trigger. Accepted messages are
immutable replay evidence. A later retention design may introduce partition
lifecycle or archival through a forward migration rather than weakening row
immutability implicitly.

Do not add a secondary index in this migration. The current application port
supports append only, and the composite primary key serves idempotency lookup.
Replay and time-range indexes will follow demonstrated query contracts.

The repository adapter must implement exact idempotency atomically: a repeated
primary key succeeds only when tenant, message ID, device, package ID, package
version, receipt time, and payload all match. Different content is an identity
collision.

## Consequences

- Durable raw frames are separated operationally from configuration metadata.
- Tenant scope is present in both message identity and device references.
- Stored messages retain the exact information required for deterministic
  package replay and observation provenance.
- Database immutability makes post-append comparison and later replay stable.
- Empty custom-protocol frames remain representable.
- The schema does not freeze a universal frame-size policy.
- Append lookup uses the primary key without speculative indexes for undefined
  replay access patterns.
- Row deletion is unavailable until retention and archival have an explicit
  operational design.

## Alternatives considered

### Store messages in the metadata schema

Configuration and ingestion facts have different write volume, retention,
backup, and access patterns. A separate namespace keeps those concerns visible
without requiring a separate database today.

### Use a database-generated surrogate identity

The caller needs a stable identity before append to retry an ambiguous result.
A database-generated sequence would require another idempotency key and would
make the accepted-message identity depend on one adapter.

### Store a payload hash instead of exact bytes

A hash can detect likely equality but cannot reproduce the original protocol
input for deterministic replay. Exact bytes are the authoritative evidence.

### Store both payload and a digest

Frames are already bounded before persistence, and the initial append operation
can compare `BYTEA` directly. A digest adds storage and collision policy before
measurements demonstrate that comparison cost requires it.

### Allow updates for operational corrections

Changing raw input or its trusted provenance would invalidate replay and derived
observation identity. Corrections belong in new facts or later semantic layers.

### Add device-time and endpoint columns

Those values are decoder output. One frame can contain several timestamps and
endpoint-scoped sources, so they do not belong on the accepted raw message.

## Out of scope

- implementing `AcceptedMessageLog` for PostgreSQL;
- exact-retry comparison queries and error mapping;
- generating accepted-message identities;
- accepted-message read and replay query ports;
- decoded-observation persistence;
- schema partitioning, retention, and archival;
- compression, encryption, and external payload storage;
- row-level security and database role design;
- protocol-package catalogue and activation foreign keys;
- production capacity limits and performance indexes.
