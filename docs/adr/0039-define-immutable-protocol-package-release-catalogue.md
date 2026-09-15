# ADR 0039: Define an immutable protocol package release catalogue

## Status

Accepted, 15 September 2026.

## Context

Runtime admission binds compatible component code to a package release and
artifact digest in memory. FleetIQ needs a storage-independent catalogue
contract before defining its PostgreSQL schema and adapter. Custom packages are
tenant-owned, and equal package labels in different tenants must not collide.

A textual package version is an opaque release label rather than content
identity. Replacing bytes beneath an existing release makes replay and audit
ambiguous. Registration must remain separate from compatibility admission and
authorization to activate code for devices.

## Decision

Define `ProtocolPackageRelease` in the application layer with explicit tenant
ownership, immutable package ID and version, a SHA-256 artifact digest, and a
nonzero raw artifact byte length.

Move `ProtocolArtifactDigest` to the infrastructure-free telemetry crate. It
stores exactly 32 bytes and provides canonical lowercase hexadecimal display.
The Wasmtime adapter continues to compute it from artifact bytes and re-exports
the type for stable runtime imports. Reconstructing stored digest bytes does not
create admission evidence.

Define an application-owned `ProtocolPackageCatalog` port. Registration is
append-only: the first exact record returns `Registered`, an identical retry
returns `AlreadyPresent`, and the same tenant/package/version key with a
different digest or byte length is an error. Lookup always requires tenant and
the exact package release.

Catalogue implementations must not implicitly admit, activate, distribute, or
execute artifacts.

## Consequences

- Custom package release labels are isolated by tenant.
- Immutable content metadata supports exact registration retry and later
  artifact-integrity checks.
- PostgreSQL can implement the port without an application dependency on
  Wasmtime.
- Artifact bytes and signatures remain outside this metadata value.
- A future workflow must prove that catalogue metadata and admitted bytes refer
  to the same artifact.
- Activation must separately authorize a tenant and device to use a registered,
  successfully admitted release.

## Alternatives considered

### Use a platform-global package release key

This requires a namespace allocation or publisher model before tenant-authored
custom packages can be represented safely.

### Treat package version as artifact identity

An opaque version cannot detect byte replacement or inconsistent distribution.

### Store runtime admission state in the catalogue record

Admission depends on host ABI and policy and may be repeated after runtime
upgrades. Combining it with immutable metadata gives one record two lifecycles.

### Authorize activation when registration succeeds

Registration does not prove compatibility, publisher trust, tenant permission,
rollout safety, or device assignment.

## Out of scope

- PostgreSQL catalogue schema and adapter;
- artifact object storage and retrieval;
- manifests, signing, and publisher trust;
- admission-result persistence;
- tenant activation permissions;
- device-to-package assignment and selection;
- session pinning, rollout, upgrade, and rollback;
- deletion, retention, and package withdrawal; and
- adding artifact identity to accepted-message persistence.
