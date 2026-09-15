# ADR 0038: Bind admitted components to release and artifact identity

## Status

Accepted, 15 September 2026.

Digest ownership and reconstruction are amended by
[ADR 0039](0039-define-immutable-protocol-package-release-catalogue.md). The digest
value can be reconstructed from stored SHA-256 bytes; runtime admission evidence
still requires the opaque admitted component created from actual artifact bytes.

## Context

Component admission proves that bytes fit the configured size, request no host
imports, and implement the exact protocol-package WIT world. The resulting
compiled value previously carried only typed pre-instantiation bindings.

Accepted messages identify the package release that decoded them. If execution
and package identity remain separate caller inputs, a caller can accidentally
record a different release from the component that actually ran. A release can
also be rebuilt or replaced while retaining its textual version. Package
catalogue, activation, rollback, and audit therefore need the identity of both
the declared release and exact admitted bytes.

## Decision

Replace the anonymous compiled component in the public API with
`AdmittedProtocolComponent`. Admission requires a `ProtocolPackageRef` and
computes SHA-256 over the exact component bytes before storing:

- the immutable package ID and version;
- the 32-byte artifact digest; and
- the typed pre-instantiation bindings produced by admission.

Expose the digest as binary bytes and canonical lowercase hexadecimal text.
Callers cannot construct a digest directly or supply it independently of the
artifact bytes.

Every public component framing and processing operation accepts the admitted
value. Successful frame-processing results and complete session turns retain the
package release and artifact digest. Consuming these results does not discard
execution identity.

## Consequences

- Runtime component execution is never anonymous.
- Package release identity cannot be selected independently at invocation.
- The artifact digest gives catalogue and audit work a stable content identity.
- Processing results provide the provenance needed to construct later durable
  acceptance context from the component that actually ran.
- SHA-256 becomes a small production dependency of the Wasmtime adapter.
- Existing callers must supply a package release during admission and consume
  the expanded processing result.
- This change binds identity in memory; it does not yet persist or authorize a
  package release.

## Alternatives considered

### Keep package identity in session configuration only

That leaves execution results structurally independent from the configuration
that selected their code and permits accidental substitution downstream.

### Trust an uploader-provided checksum

The host already receives the artifact bytes and can compute their identity.
Trusting separate text creates another mismatch to validate.

### Use package version as content identity

Versions are opaque release labels. A cryptographic digest detects replacement
or inconsistent distribution of bytes under the same label.

### Store the complete artifact in each processing result

That would copy and retain potentially large untrusted bytes on the message path.
The admitted compiled value owns bindings; results need only stable identity.

## Out of scope

- package manifest fields and signing;
- artifact storage and distribution;
- catalogue persistence and release uniqueness;
- tenant activation permissions;
- device-to-package selection;
- session pinning, rollout, upgrade, and rollback policy;
- compilation isolation, concurrency, caching, and eviction; and
- adding the artifact digest to accepted-message persistence.
