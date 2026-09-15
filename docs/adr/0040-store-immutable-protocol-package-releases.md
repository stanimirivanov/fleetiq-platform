# ADR 0040: Store immutable protocol package releases

## Status

Accepted, 15 September 2026.

## Context

The application catalogue contract defines a tenant-scoped immutable package
release with a SHA-256 artifact digest and unsigned 64-bit byte length. Durable
storage must enforce tenant isolation, valid identifiers, exact digest shape,
the complete byte-length range, and append-only behavior under concurrent or
non-application writes.

PostgreSQL `BIGINT` cannot represent values above signed `i64::MAX`. A constrained
`NUMERIC(20, 0)` accepts the required maximum but rounds fractional input before
a check constraint can reject it. Digest text would also permit multiple
encodings of the same bytes.

Existing accepted messages already contain package labels without catalogue
references. Adding a foreign key now would require a migration policy for those
historical rows and would still not prove that a package was authorized for the
device session.

## Decision

Create `metadata.protocol_package_releases` with a composite primary key over
tenant ID, package ID, and package version. Reference `metadata.tenants` through
the tenant key and use the shared identifier validation function for both
package fields.

Store SHA-256 as `BYTEA` with an exact 32-byte check. Store artifact byte length
as unconstrained `NUMERIC`, then require scale zero and a value from 1 through
`u64::MAX`. This rejects fractional input without rounding and preserves the
application contract's complete range.

Reject `UPDATE` and `DELETE` through a table trigger. Exact retry and content
collision classification remain adapter responsibilities because a primary-key
conflict alone cannot compare all immutable fields.

Do not add a foreign key from accepted messages in this migration. Catalogue
registration remains independent from admission and activation.

## Consequences

- PostgreSQL enforces tenant ownership and release-key uniqueness.
- Invalid identifiers, non-32-byte digests, zero, negative, fractional, and
  oversized artifact lengths cannot become durable catalogue records.
- Two tenants can use equal package IDs and versions independently.
- Release content cannot be replaced or deleted through ordinary row mutation.
- The future adapter must compare digest and byte length after a key conflict.
- The future adapter must convert integral PostgreSQL `NUMERIC` safely to `u64`.
- Existing ingestion rows and tests need no package catalogue fixtures yet.
- Direct database privileges and row-level security remain deployment concerns
  shared with the existing metadata schema.

## Alternatives considered

### Store artifact length as `BIGINT`

This loses the upper half of the application contract's unsigned 64-bit range.

### Store artifact length as `NUMERIC(20, 0)`

PostgreSQL coerces fractional values to the declared scale before constraints
inspect them, allowing silent rounding instead of rejection.

### Store SHA-256 as hexadecimal text

Binary storage represents one canonical value and avoids validating case,
prefix, and character choices in the database.

### Reference the catalogue from accepted messages immediately

That couples registration to ingestion before activation semantics and a
historical-row migration policy exist.

## Out of scope

- PostgreSQL catalogue adapter and row reconstruction;
- exact retry and identity-collision classification;
- accepted-message catalogue foreign keys;
- artifact bytes, object storage, and integrity verification on retrieval;
- package manifests, signatures, and publisher trust;
- admission-result persistence;
- tenant and device activation;
- rollout, session pinning, upgrade, rollback, and withdrawal; and
- database roles, grants, and row-level security policy.
