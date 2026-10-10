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
- OpenAPI tooling requires Node.js 22.12 or newer and pins pnpm and Redocly CLI in `package.json`
  and `pnpm-lock.yaml`; its separate CI job checks the contract bundle.
- Secret protection uses checksum-verified Gitleaks 8.30.1; its exact version
  and exception policy are maintained with the [protection guide](harness-protection.md).
- Dependabot proposes weekly Cargo, npm, GitHub Actions, and container updates.
  Review and verify each proposal; an automated update is not an approval.

## Run the checks

Install the same CLI versions used by CI with the pinned Rust toolchain:

```powershell
cargo install cargo-deny --version 0.20.2 --locked
cargo install cargo-machete --version 0.9.2 --locked
```

From the workspace root, run:

```powershell
cargo deny check
cargo machete --with-metadata --skip-target-dir
```

`cargo-deny` checks the complete locked dependency graph against `deny.toml`.
It fetches the current RustSec advisory database. A network or database-fetch
failure means the check did not run; report that condition and rerun when the
database is available. CI runs the commands in a separate `Supply-chain
policy` job after the repository and architecture checks.

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
