# Dependency management and supply-chain checks

## TL;DR

- `cargo xtask architecture` checks direct dependency ownership and inherited
  workspace lints before supply-chain review; it rejects nonmember paths.
- The locked Cargo workspace must pass `cargo deny check` and
  `cargo machete --with-metadata --skip-target-dir` before handoff.
- `deny.toml` rejects known vulnerabilities, unapproved licenses and sources,
  yanked packages, and unused or inconsistent workspace dependency declarations.
- The tools are pinned to `cargo-deny` 0.20.2 and `cargo-machete` 0.9.2. The
  advisory check needs a current RustSec database and network access.
- OpenAPI verification requires the inventoried Node.js 24 major and pins pnpm and Redocly CLI in `package.json`
  and `pnpm-lock.yaml`; its separate CI job checks the contract bundle.
- Secret protection uses checksum-verified Gitleaks 8.30.1; its exact version
  and exception policy are maintained with the [protection guide](harness-protection.md).
- Dependabot proposes weekly Cargo, npm, GitHub Actions, and container updates.
  Review and verify each proposal; an automated update is not an approval.

## Run the checks

Install the same CLI versions used by CI with the pinned Rust toolchain:

```bash
cargo install cargo-deny --version 0.20.2 --locked
cargo install cargo-machete --version 0.9.2 --locked
```

From the workspace root, run:

```bash
cargo deny check
cargo machete --with-metadata --skip-target-dir
```

`cargo-deny` checks the complete locked dependency graph against `deny.toml`.
It fetches the current RustSec advisory database. If a prerequisite prevents the
command from starting, record **not run**. If the started command fails to fetch
the database, record **failed** with the operational blocker and incomplete
review; that status is neither a passing review nor proof of a vulnerability.
Rerun when advisory data is available. The
[verification runner](verification.md) preserves these statuses. CI runs the
same supply-chain group in a separate `Supply-chain policy` job after the
repository and architecture checks.

`cargo-machete` is a fast heuristic for unused direct dependencies. Its
metadata mode resolves renamed packages and target-specific declarations, but
it cannot prove that every dependency is used or unused. Investigate each
finding against source, tests, generated code, and build scripts. If a real
usage is invisible to the tool, add a narrow `package.metadata.cargo-machete`
exception beside the affected manifest with a reason. Do not use `--fix` in CI.

## Policy and exceptions

`deny.toml` permits only the listed SPDX license choices and the crates.io
registry. It rejects unknown Git and registry sources. The license check
includes build and development dependencies because their code runs in build,
test, and CI environments. Unpublished FleetIQ crates are excluded from this
third-party license policy. Duplicate transitive versions are warnings for
review; they can be required by unrelated upstream version constraints.
Duplicate external workspace dependency declarations and unused workspace
declarations are errors. `cargo xtask architecture` independently checks exact
owning packages and normal, development, or build kinds for both internal and
third-party dependencies. It rejects nonmember paths, root source overrides,
and missing workspace lint inheritance. See the canonical [dependency rules](../architecture/dependency-rules.md)
for policy updates; approval of ownership never approves a supply-chain source
or advisory exception.

Do not suppress a new advisory or widen the license/source allowlist merely to
make the gate green. Record the affected package, advisory or license, actual
usage path, available upgrade or replacement, exposure, owner, and removal
condition in the dependency PR. Any temporary exception must be exact and
documented next to the `deny.toml` entry. A vulnerability in reachable runtime
code should be resolved before merge unless an explicit security decision says
otherwise.

When adding or updating a dependency, use the [PR template](../../.github/PULL_REQUEST_TEMPLATE.md)
to explain the capability it supplies, layer that owns it, alternatives,
features, MSRV, license and source, maintenance and security status, graph
change, and exit path. Keep runtime features narrow and verify the full Rust
gate after changing the lockfile.

## Update proposals

[Dependabot](../../.github/dependabot.yml) checks Cargo and npm manifests and the
lockfiles, GitHub Actions, and the local Docker inputs weekly. It limits open
version-update PRs so security work and capability changes remain reviewable.
Each update follows the normal PR gate and must preserve architecture
boundaries and protocol fixture compatibility.


### Rust toolchain and builder updates

A Rust builder-image update must be coordinated with `rust-toolchain.toml`,
workspace `rust-version`, the verification catalogue, current development
instructions, and regenerated CI. Keep the immutable image digest and compiler
version consistent; do not bypass the producer-policy check to accept an isolated
image update. Historical qualification records retain the compiler actually used.

Run the xtask sensor tests, producer-policy fixtures, CI consistency check and
Rust verification with the new compiler. Producer-test mutations derive their
baseline from the canonical pin and must change the Dockerfile before asserting
rejection, so a version bump cannot turn a negative fixture into a no-op.

### GitHub Actions updates

Action major tags live only in the workflow. The template has no duplicate
version list to maintain, and minor and patch releases do not require workflow
edits. Review major-version changes and runtime compatibility, then run the
applicable checks. See the canonical
[CI resilience rules](verification.md#ci-resilience-rules) for tag validation,
structural regeneration, tooling isolation, and cache policy.

### Wasmtime security patch qualification

The protocol adapter retains the same Wasmtime features and owning layer, with
minimum version 49.0.5 and coordinated Cranelift 0.136.5/internal runtime patches.
This fixes the three advisories published on 29 October 2026:

- [GHSA-3982-jqvv-277m](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-3982-jqvv-277m): insufficient typed-function checks for i31/nullref.
- [GHSA-qw58-7h65-crq2](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-qw58-7h65-crq2): cross-engine struct/supertype confusion.
- [GHSA-6fr9-xmw9-5352](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-6fr9-xmw9-5352): uncharged memory for borrowed component resources.

The [49.0.5 release](https://github.com/bytecodealliance/wasmtime/releases/tag/v49.0.5)
also retains the earlier 49.0.4 fixes for RUSTSEC-2026-0378/0379. The owner remains
`fleetiq-protocol-wasm`; alternatives are affected 49.0.4 or a broader major-version
migration, so this is the narrow compatible patch. Features remain `anyhow`,
`component-model`, `cranelift`, `runtime` and `std`, with defaults disabled. Upstream
MSRV is 1.96.0, within the pinned project compiler 1.99.1; license remains
`Apache-2.0 WITH LLVM-exception` from crates.io. The maintained upstream patch
updates 27 existing package versions, adding/removing no package names.

No WASI/network capability, new direct dependency, advisory exception or policy
weakening is added. Existing protocol admission, fuel, memory, deadline,
cancellation and cleanup tests qualify the patch, followed by the full workspace
gate and authoritative supply-chain group. Future advisories still fail cargo-deny.
Rollback must use a qualified patched successor, never restore the vulnerable runtime.

The protocol component fixture parser and encoder (`wit-parser` and
`wit-component`) must pin the same exact wasm-tools release. The repository
architecture check enforces that requirement, and Dependabot groups their
updates with the `wat` fixture compiler for joint review. The parser's
`std` feature enables the in-memory `Resolve::push_str` fixture helper.
`wat` may use a neighboring wasm-tools release only when the complete
`fleetiq-protocol-wasm` fixture suite passes and the resolved `wasmparser`
and `wasm-encoder` versions have been reviewed. These dependencies build test
fixtures; they do not change the production protocol package contract.

## Secret detector tooling

Gitleaks is an external development tool rather than a Rust production library.
`node scripts/install-gitleaks.mjs` installs official 8.30.1 archives after
verifying reviewed platform-specific SHA-256 digests. The Rust wrapper rejects
other versions; update both pins together in a dedicated reviewed change and
run `cargo xtask secrets --fixtures` plus the current/base-qualified scan.
The upstream project is feature-complete and maintains fixes; a reviewed pin
still requires monitoring releases and maintenance status.

The wrapper uses the existing maintained TOML parser to validate exact scanner
policy and the existing workspace serde_json dependency to parse detector
reports safely. serde_json gains only a direct xtask tooling ownership edge;
the lockfile adds no package version or production edge. Neither parser is
reimplemented. Exact nonproduction scanner exceptions and missing-tool handling
are defined in [harness protection](harness-protection.md), separately from
RustSec advisory exceptions.

## Browser identity dependencies

The OIDC boundary uses openidconnect for protocol and ID-token verification,
reqwest/rustls through its supported HTTP adapter, cookie for parsing and cookie
attributes, OS getrandom for opaque credentials, and the existing SHA-256
implementation for credential lookup digests. It does not implement JWT crypto
or an identity provider. Version-scoped TLS license entries and the approved
verification-only RSA advisory assessment are documented in
[the dependency review](plans/browser-identity-dependency-review.md).

The approved RSA exception is enforced by cargo-deny's supported wrapper/version
bans and the architecture check's exact lockfile sensor, including prereleases
and package sources. Keep both guards with the exception. The harness uses the
maintained TOML parser; no parser or crypto dependency enters the domain crates.


### Certificate verification security patches

D06 publication qualification found RUSTSEC-2026-0382 and RUSTSEC-2026-0381 in
the previously locked provider HTTPS verification path. The lockfile selects
`rustls-webpki` 0.103.16 and `webpki-roots` 1.0.10 to reject distrusted emSign
intermediates/remove their affected public trust anchor. See the upstream
[webpki change](https://github.com/rustls/webpki/pull/546) and
[root inventory change](https://github.com/rustls/webpki-roots/pull/133).
Keep the existing OIDC transport ownership, features and additional-CA contract;
no direct dependency, source exception or advisory waiver is added.

The existing exact-version ISC and CDLA-Permissive-2.0 license selectors move
only to these patched versions. Comparing the published crate `LICENSE` files
confirmed byte-identical terms for each old/new pair:

| Package pair | LICENSE SHA-256 (both versions) |
| --- | --- |
| rustls-webpki 0.103.15 / 0.103.16 | `5b698ca13897be3afdb7174256fa1574f8c6892b8bea1a66dd6469d3fe27885a` |
| webpki-roots 1.0.9 / 1.0.10 | `e271993808fec50ab29350b39539cdec611a9103f827e0aa26d61da70e2d33f8` |

The old selectors are removed; the global license allowlist and every advisory
exception remain unchanged. Requalify the actual dependency graph with
cargo-deny and the full Rust/PostgreSQL gate; do not roll back to a vulnerable
trust store to work around provider certificate problems.
