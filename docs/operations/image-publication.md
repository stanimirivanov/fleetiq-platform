# Backend image production and publication

## TL;DR

The platform repository builds and qualifies one linux/amd64 OCI artifact.
PR CI never publishes. A current repository admin/maintainer manually requests
publication of the exact current merged main revision after PR CI passes.
Consumers select the emitted GHCR digest, not a discovery tag or source checkout.
No Kubernetes, environment promotion or automatic rollback is performed.

## Ownership and prerequisites

The root [Dockerfile](../../Dockerfile) is the sole production backend package.
[ADR 0110](../adr/0110-publish-qualified-immutable-backend-artifacts.md) records
artifact identity, authority and limitations. D03 owns UX publication; D04/D05
own runtime packages and reviewed environment selection. Transitional deployment
Compose packaging remains a diagnostic until D12 removes competing release paths;
it must not introduce another backend publisher.

Use Node.js 24, Git, Docker with the Linux daemon, Compose 2.24.4 or newer and
Buildx with OCI directory export/attestation support. The actual qualified local
versions and pinned image tools are recorded with PR evidence. Docker Desktop's
Linux context is supported; Docker Engine on Linux is used by producer CI.
No local Kubernetes context or registry credential is needed for qualification.
The cold build has a 60-minute deadline; the image jobs allow 90 minutes for
building, scanning, runtime/registry checks and cleanup. This is an upper bound,
not a retry or additional workflow run.

The repository/package administrator must allow GitHub Actions package writes,
retain selected image digests, and restrict package deletion/retagging. GHCR packages
start private by default; confirm visibility and source-repository access before
consumption. Publication uses the job-scoped GITHUB_TOKEN, not the deployment
source-read PAT. Never paste credentials into commands, build arguments or issues.

## Qualify locally or in PR CI

From a clean reviewed checkout, or a dirty candidate explicitly labelled as such:

```powershell
node --test scripts/images/tests/*.test.mjs
node scripts/images/qualify.mjs
```

If Docker is not on PATH, set FLEETIQ_DOCKER to its executable path. The script
creates a uniquely owned builder, ephemeral PostgreSQL/registry containers and
private network. Runtime archive spooling uses bounded container scratch space
and native binary output from Docker exec, avoiding slow Windows bind-mount
writes and the daemon archive API's process-tmpfs limitation; the tool
container has no network or daemon socket. Linux tools use the host UID/GID to
read owner-only archives and write owned evidence without adding capabilities or
making files public. Trivy scans the original OCI layout at the exact runtime
manifest digest before Docker loading, as supported by its
[OCI layout input](https://trivy.dev/docs/v0.76/guide/target/container_image/#oci-layout).
A small single-runtime OCI entrypoint handles Trivy's nested-index selector
limitation; it mounts the original blobs read-only and leaves the publication
layout unchanged. The scanner never reads a Docker-save archive. Docker read-back
remains only for
raw config-digest verification of the runtime fixture. The observed local image
ID is tracked separately for runtime and cleanup: classic and containerd image
stores
use different ID semantics. The reviewed OCI publication digest is unchanged.
HTTP binds only to an allocated loopback port; the existing port 5432, developer
database and Kubernetes cluster are not used. Every transient
container, including tools, receives an owner label. Cleanup verifies ownership;
there is no global prune, volume reset or shared-service repair.

Without an imported cache, a completed build exports an ignored `build-cache`
beside its layout so failed
scans or runtime checks do not force another cold compilation. It is never
uploaded as qualification evidence.

For a trusted local Buildx OCI cache exported under this checkout's ignored
`target` directory, optionally set `FLEETIQ_IMAGE_BUILD_CACHE` to that directory.
Links and paths outside `target` are rejected. CI/publication does not restore a
PR-supplied cache. An imported cache is read-only input and is not redundantly
exported. Cache hits never replace metadata, scan or runtime checks.

Qualification assigns a unique local-only image reference so BuildKit populates
statement subjects; this does not push a tag. It builds once, exports OCI
metadata/SBOM/provenance, verifies the
actual image, scans its runtime archive, then tests empty/stale-schema refusal,
explicit migration, a no-DDL runtime credential, health/discovery, nonroot/read-only
execution, SIGTERM, exact inventory and real development Compose. It roundtrips
the complete index through an isolated registry without a second build.

Ignored `target/images/run-<uuid>/reports/qualification.json` remains incomplete
on failure. It becomes passed only after all checks and cleanup. Reports include
hashes and bounded metadata, not credentials or captured process logs. Secret
findings remain failing but their matched bytes/code are not uploaded. Raw image
layouts are ignored local outputs. The fresh vulnerability database uses bounded
Linux scratch storage (4 GiB ceiling; the measured database required about
1.5 GiB) and is removed with its owned scanner container; retained
reports contain its version and freshness metadata. These are not Git artifacts.
The complete thirteen-command platform gate is also required; this artifact gate
does not replace Rust/PostgreSQL or cargo-deny checks.

## Minimal runtime and readiness

The digest-pinned Distroless cc-debian13 runtime supplies glibc, libgcc and
certificate data without a shell, package manager or curl. A general Debian
runtime failed the fresh scan on high-severity packages that the backend does
not need; removing those packages preserves the same vulnerability policy.
The source and builder remain separate from the runtime.

`fleetiq-server healthcheck` probes only `http://127.0.0.1:8080/health/ready`.
It requires exactly HTTP 204, refuses redirects and proxy configuration, and
bounds the entire HTTP request to two seconds. It reads no database or identity
configuration and prints only a bounded failure message. The image and local
Compose use this command; Kubernetes can use the same HTTP endpoint directly.
Inspect runtime inventories through `migration-inventory`; do not expect a
shell or diagnostic tools inside the production container.

## Request an immutable publication

After this implementation merges and the relevant PR CI passes:

1. Use GitHub's **Publish backend image** workflow on branch **main**.
2. Enter the complete lowercase 40-character current main commit as source_revision.
3. Run the workflow using a current repository admin/maintainer account.
4. Inspect the prerequisite, qualification, scan and registry-readback results.
5. Retain the publication receipt and select its digest through the deployment
   repository's separately reviewed promotion process when D05 is available.

The manual dispatch is the human publication authorization. Both the original
actor and a rerun's triggering actor must currently be admin/maintain. Ordinary
write/read/triage access cannot authorize publication. Main must correspond to a
merged repository PR whose latest PR CI run succeeded. Its server-recorded CI
run name binds the PR number, full head revision, main target and exact merge
checkout even after GitHub removes the run's PR association on merge. The tested
checkout's Git tree must exactly equal the final merged source tree; unavailable
objects or a base-branch change that changes source bytes refuse publication. Advancing main while a
build runs makes final authorization stale; request its new exact revision.

The workflow has no push/tag/PR publication trigger. It serializes publication
runs without cancelling another release and checks authority again before writes.
The upload job uses packages:write and read-only source/CI/PR access; no contents
write, environment deployment token, infrastructure permission or cluster access
is granted. No paid private environment-review feature is required. This does not
claim independent two-person approval; D13 owns multi-user release authority.

## Consumer contract and provenance

A successful publication emits:

- `ghcr.io/stanimirivanov/fleetiq-platform@sha256:<index digest>` and linux/amd64.
- Exact source revision, qualification fingerprint, workflow run/attempt and PR CI.
- Platform-manifest/config identities, SPDX and SLSA statement hashes.
- Migration inventory SHA-256, complete count and SHA-384 source checksums.
- Fresh scan metadata and hashes tied to the tested runtime artifact.

The unique `build-<source>-<run>-<attempt>` reference is discovery metadata. It is
never reused by the publisher, and no latest, main or movable release tag exists.
Only a digest is an execution contract; registry administrators could still alter
tags or delete artifacts. Reconcile selected digest retention with D05/D07 policy.

Run the exact image's `migration-inventory` without a database, or inspect
`/usr/share/fleetiq/migrations.json` inside it. Both are checked against source
and the binary. Production defaults to checked serving; release orchestration
must run explicit migration/bootstrap first with separate credentials as specified
in [runtime and migrations](runtime-and-migrations.md).

SBOM and max-mode SLSA v1 provenance remain attached to the OCI index through Skopeo's
all-platform/digest-preserving copy. They are unsigned BuildKit statements: their
hashes detect substitution, not a forged builder identity. Verify registry/source
publisher authority independently. D05 must bind its selected artifact and review
proof, not treat a plausible JSON record as an approval.

The runtime SBOM report covers installed Debian packages. The attached builder
SBOM also records declared workspace Cargo.lock dependencies through the explicit
Rust lockfile cataloguer, alongside the existing image/file/SBOM catalogues.
Qualification rejects missing Debian or Rust coverage. These declarations are
not a claim about the exact linked Rust feature subset; cargo-deny independently
checks the reviewed locked source graph. All statements remain in the OCI index.

PR diagnostic artifacts are retained for 14 days; publication reports for 90 days.
The registry-attached statements and embedded inventory persist with the image.
Copy required qualification/review evidence into deployment-owned durable records
before artifact expiry. No indefinite artifact availability or production
readiness is implied by this producer.

## Scanning and tool updates

[tools.json](../../scripts/images/tools.json) pins BuildKit 0.34.0, the BuildKit Syft
scanner 1.12.0, the official Skopeo distribution (verified 1.22.3), Trivy 0.76.0 and disposable PostgreSQL/registry images
by digest. The Dockerfile separately owns compiler/runtime/frontend digest pins.
GitHub Actions retain major references, as required by repository policy.

These are maintained upstream Apache-2.0/MIT or upstream container distributions;
Docker/BuildKit supplies building/SPDX/SLSA, Skopeo supplies OCI transport and
Trivy supplies vulnerability/secret analysis. They add no Rust/runtime dependency.
Review official release provenance, supported platform, digest and compatibility
before changing a pin; rerun policy mutations, the real image gate and full
platform acceptance. Remove tooling when its capability is replaced, not its checks.

Trivy uses a current official schema-v2 database, at most 24 hours old and before
its next-update deadline. High/critical vulnerabilities fail even when unfixed.
No ignore file, advisory waiver, insecure public-registry TLS or absent-tool pass
is added. Only the disposable private registry uses HTTP within its owned network.
The source's cargo-deny exception policy remains unchanged and separately enforced.

Runtime updates require a reviewed Distroless digest replacement. Builder
repositories and upstream inputs still mean builds are not claimed to be byte
reproducible. The tested OCI digest identifies the actual selected bytes.
The upstream Trivy supply-chain incident is a reason to retain reviewed image
digests and avoid mutable Trivy action tags, not to disable scanning; see the
[upstream advisory](https://github.com/aquasecurity/trivy/security/advisories/GHSA-69fq-xp46-6x23).

## Failure and recovery

A missing tool, incompatible source, invalid inventory, stale/failed scan or
cleanup failure prevents success. Inspect the named stage and redacted reports;
fix the cause and rerun the complete affected gate. Never change a threshold,
checksum, success flag or source identity to make a receipt pass.

A failed registry copy/readback can leave orphaned content. It receives no
successful publication receipt and must not be promoted. Diagnose access/network
state, then request a new reviewed run; do not overwrite a prior publication.
Only an explicitly authorized registry administrator handles retention/deletion.
Existing deployments, data and selected artifacts remain unchanged.
