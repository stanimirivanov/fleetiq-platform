# ADR 0110: Publish one qualified immutable backend artifact

## Status

Proposed, 18 October 2026.

Implements Kubernetes delivery-plan D02, issue #304;
acceptance awaits human review.

Supersedes: none. Superseded by: none.

## Context

Kubernetes consumers need a source-owned image rather than a deployment checkout
compiling application source. Its schema commands, migration checksums, platform,
source and build evidence must describe the same bytes. The private repository
uses PR-only CI to limit cost; publishing on every merge would duplicate work.

## Decision

The root Dockerfile owns the production backend image. Pin compiler/runtime
bases and operational tool images by digest, align Rust with rust-toolchain.toml,
and initially qualify only linux/amd64. Default to `serve --check-schema`;
development Compose explicitly selects `serve` for migration compatibility.
The binary and its D01 migration inventory are included in the image.

BuildKit produces one OCI layout with max-mode SLSA provenance and SPDX SBOM.
Include the builder stage and explicit Rust lockfile cataloguer in SBOM scope,
preserving existing OS/file/SBOM catalogues. Declared workspace dependencies
are build inputs, not an exact linked-feature inventory. Skopeo selects a Docker runtime archive
for execution while preserving the complete original OCI index for publication.
Verify the loaded config digest, actual source inventory and attestation subjects.
Use the Distroless cc-debian13 runtime with only the native libraries the Rust
binary requires. The fresh scan rejected the general Debian/curl runtime on
unnecessary high-severity packages; no scan exception is introduced. A bounded
`healthcheck` command reuses the existing HTTP client, probes only container
loopback port 8080 and requires exactly HTTP 204 without redirects or proxies.

Exercise the image with isolated PostgreSQL, restrictive runtime settings,
SIGTERM, development Compose and a digest-preserving disposable registry roundtrip.
A fresh Trivy database scans runtime packages and secrets; high/critical findings,
including unfixed vulnerabilities, fail. Cargo supply-chain checks remain separate.

Producer evidence records the index/platform/config digests, exact source and
reviewable-file fingerprint, inventory/scan/attestation hashes and every required
check. Success is recorded only after owned resource cleanup. Secret matches are
redacted from failure reports without removing findings or making them pass.

Publication is a separate manual main-only workflow. Verify the initiating and
rerunning actor's current admin/maintain permission, exact current main commit,
merged source PR and latest successful PR CI. Bind its exact checkout in the CI
run name and compare its Git tree with merged main, rejecting untested merge
results. Revalidate before registry writes.
No Enterprise-only environment review feature, new PAT or deployment credential
is required. This is maintainer authorization, not independent two-person approval;
that later release authority belongs to the deployment plan's multi-user slice.

Copy the qualified layout without rebuilding, preserving all digests and attached
statements. Refuse existing discovery tags and verify registry readback. Unique
run/attempt references aid discovery; consumers select `repository@sha256:...`.
The publisher has packages:write but no Git write or Kubernetes authority.

BuildKit statements are content-bound unsigned provenance, not a cryptographic
trusted-builder signature. Registry access and reviewed workflow authority govern
this producer. D05 must independently verify producer/review evidence and select
artifacts; schema validity alone cannot authorize promotion. GitHub qualification
reports expire after 90 days; registry-attached SBOM/provenance and embedded
inventory persist with their image. Deployment evidence retention remains D05/D07.

## Consequences

A PR qualifies actual production bytes and development composition in one build.
It does not publish, and merging does not start another CI run. Initial cold
image compilation/scan may be expensive; optional caches can never skip checks.

Compiler/runtime updates require both image and workspace qualification. Runtime
updates require a new reviewed Distroless digest; upstream builder inputs still
mean independently rebuilding a
commit may yield a different digest. This decision guarantees immutable selection
of one tested artifact, not byte-reproducible builds.

A failed push/readback may leave an unqualified registry artifact. No publication
receipt is emitted; it must not be promoted. Automatic deletion, retagging,
rollback and environment changes are not recovery. Registry administrators must
retain selected digests and restrict package write/delete permissions.

The transitional deployment repository's Compose packaging remains until its
planned cutover; it does not become a second producer or publication authority.

## Alternatives considered

- Rebuild during deployment or after tests: breaks the tested-artifact identity.
- Load-only Docker output: loses portable OCI attestation content on classic stores.
- Publish on push to main: adds redundant CI cost and removes explicit release intent.
- Mandatory private environment reviewers: requires an Enterprise plan; unnecessary
  for this single-maintainer producer and distinct from future multi-user releases.
- Custom builder, OCI transport or vulnerability scanner: maintained upstream
  tools already supply these capabilities.

## References

- [D01 runtime and migrations](../operations/runtime-and-migrations.md)
- [Image publication procedure](../operations/image-publication.md)
- [Docker OCI exporter](https://docs.docker.com/build/exporters/oci-docker/)
- [Docker attestations](https://docs.docker.com/build/metadata/attestations/)
- [Skopeo](https://github.com/containers/skopeo)

- [Distroless Rust runtime contents](https://github.com/GoogleContainerTools/distroless/blob/main/cc/README.md)
