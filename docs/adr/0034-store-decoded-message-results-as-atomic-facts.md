# ADR 0034: Store decoded message results as atomic facts

- Status: accepted
- Date: 2026-09-15

## Context

The application-owned `DecodedObservationLog` treats the complete ordered
output of decoding one accepted message as an atomic and idempotent persistence
unit. PostgreSQL needs to represent both nonempty output and successful decoding
that emits no observations, preserve deterministic output identity, and reject
cross-tenant or cross-package references.

Trusted tenant, communication-device, host receipt-time, message, and package
provenance already exists in the immutable accepted-message row. Repeating all
of it on every observation would create several independently stored copies of
the same facts. The schema still needs a structural way to prove that a result
uses the package release pinned when the message was accepted.

Decoded values form a closed initial set of Boolean, signed integer,
arbitrary-precision decimal, and text. A database row must retain the exact type
without coercing every value to text or an untyped document. Device-recorded
time and source unit are optional with distinct absence semantics.

## Decision

Add two append-only tables to the existing `ingestion` schema.

`ingestion.decoded_message_results` stores one completion marker with:

- tenant-scoped accepted-message identity;
- exact protocol-package identity and version; and
- the complete observation count, including zero.

Use `(tenant_id, message_id, protocol_package_id,
protocol_package_version)` as the result primary key. Add a matching redundant
unique constraint to `ingestion.accepted_messages` and reference it from the
result marker. This composite path proves in PostgreSQL that the result uses the
same package release as the accepted raw message. The existing accepted-message
primary key already guarantees uniqueness by tenant and message, so adding the
expanded constraint requires no data correction.

`ingestion.decoded_observations` stores decoder-owned output fields:

- zero-based output index;
- endpoint and source-signal identifiers;
- optional device-recorded Unix milliseconds;
- one typed value;
- optional source unit; and
- source-reported quality.

The child primary key extends the complete result identity with
`output_index`. A composite foreign key requires the parent result marker.
Admitted device identity and host receipt time remain normalized in the
accepted-message row and are reconstructed through the parent rather than
duplicated on every observation.

Represent values with a discriminator and nullable Boolean, `BIGINT`,
`NUMERIC`, and text columns. A check constraint requires exactly the column
selected by the discriminator. Further checks allow units only on integer and
decimal values and restrict quality to `good`, `uncertain`, or `invalid`.
PostgreSQL `NUMERIC` preserves exact decimal arithmetic and maps to the selected
Rust `BigDecimal` integration.

Use `BIGINT` for output positions and counts, with checks matching the unsigned
32-bit identity range. One result can contain positions from zero through
`2^32 - 1`, so its maximum count is `2^32`.

Reject row-level updates and deletes on both tables. Retention and reprocessing
will use explicit future lifecycle designs rather than modifying derivation
evidence.

The application adapter must insert the marker and complete observation
collection in one transaction. Primary and foreign keys prevent duplicate or
orphan positions. Do not add an aggregate constraint trigger to count all child
rows after every insertion: it would turn a large batch into repeated scans.
The validated application batch and transaction establish completeness, and
the marker's count supports exact-retry comparison.

Do not add time, device, signal, or endpoint indexes yet. This increment defines
the append boundary; indexes will follow concrete replay and query contracts.

## Consequences

- Successful zero-output decoding has a durable row.
- A result cannot reference another tenant's message or a package release that
  differs from the accepted message.
- Output position remains stable and unique within one decoding result.
- Typed values can be reconstructed without guessing from serialized text.
- Optional device time remains distinct from host receipt time.
- Accepted-message provenance is stored once and reached through composite
  references.
- Atomic completeness depends on the repository transaction required by the
  application port; database clients must use that boundary for appends.
- The accepted-message table gains a redundant unique index. Installing it
  takes PostgreSQL's normal `ALTER TABLE` lock and must use the schema-change
  window once the table is high volume.
- Query-oriented indexes remain deferred until access patterns are defined.

## Alternatives considered

### Store every result as a JSON document

JSON would make typed constraints, exact numeric handling, ordered identity,
and common relational queries harder to enforce and optimize.

### Store only observation rows

A message producing no observations would leave no evidence that decoding
completed successfully.

### Duplicate all accepted-message fields on every observation

This increases ingestion storage and allows redundant trusted provenance to
diverge unless every copy participates in larger constraints.

### Reference only tenant and message identity

The result's package columns could then disagree with the immutable release
pinned to the accepted message unless a trigger performed the comparison.

### Validate child count with a deferred row trigger

A trigger fired for every observation would repeatedly count a growing batch.
For devices producing hundreds or thousands of outputs, that creates quadratic
work at commit. The repository already owns atomic validated batch insertion.

### Store every value as text

This loses database-level type integrity and makes numeric filtering and
aggregation depend on repeated parsing and failure policy.

## Out of scope

- implementing `DecodedObservationLog` for PostgreSQL;
- exact-retry queries and identity-collision error mapping;
- read and replay query ports;
- semantic signal binding and asset-property change persistence;
- publishing decoded observations;
- result timestamps, processing duration, and decoder diagnostics;
- partitioning, retention, archival, and compression;
- row-level security and database role design; and
- query-specific secondary indexes.
