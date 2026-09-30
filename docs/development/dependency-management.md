# Dependency management and supply-chain checks

## TL;DR

- The locked Cargo workspace must pass `cargo deny check` and
  `cargo machete --with-metadata --skip-target-dir` before handoff.
- `deny.toml` rejects known vulnerabilities, unapproved licenses and sources,
  yanked packages, and unused or inconsistent workspace dependency declarations.
- The tools are pinned to `cargo-deny` 0.20.2 and `cargo-machete` 0.9.2. The
  advisory check needs a current RustSec database and network access.
- Dependabot proposes weekly Cargo, GitHub Actions, and container updates.
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
declarations are errors. Local path edges remain explicit and are checked by
`cargo xtask architecture`.

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

[Dependabot](../../.github/dependabot.yml) checks Cargo manifests and the
lockfile, GitHub Actions, and the local Docker inputs weekly. It limits open
version-update PRs so security work and capability changes remain reviewable.
Each update follows the normal PR gate and must preserve architecture
boundaries and protocol fixture compatibility.

The `wat` fixture compiler is pinned to the `1.254` wasm-tools generation used
by `wit-parser` and `wit-component`. An update that changes one of these tools
must verify the complete `fleetiq-protocol-wasm` fixture suite and examine the
resolved `wasmparser` and `wasm-encoder` versions. This pin applies only to test
fixtures; it does not change the production protocol package contract.
