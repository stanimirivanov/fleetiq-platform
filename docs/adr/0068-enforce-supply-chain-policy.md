# ADR 0068: Enforce supply-chain policy

## Status

Accepted, 30 September 2026.

Acceptance reconciled from the approved architecture and harness sequence and
merged PR #183, commit `3c43390c6383a8d1b8066ade297a90c58af97803`.
This records merge evidence; it does not claim independently enforced review
settings or infer acceptance from passing tests.

Supersedes: none. Superseded by: none.

## Context

ADR 0066 checks direct dependencies between FleetIQ workspace packages, and
ADR 0067 checks repository documentation and planning structure. Neither
checks third-party advisories, licensing, source provenance, or unused direct
dependencies. The workspace uses SQLx, Wasmtime, WIT tooling, and generated
test fixtures, so the full locked graph is substantially larger than the
direct dependency list.

The `wat` fixture declaration permitted any compatible `1.x` version. Before
this change, the lockfile resolved its parser and encoder to the `0.259`
generation while the WIT component tooling resolved `0.254`. A fixture could be encoded by
a newer toolchain than the one that validates the component contract.

The project needs a reproducible supply-chain check before capability
extraction introduces more dependencies. Advisory data is time-sensitive, so
the check cannot be made entirely offline.

## Decision

Use pinned `cargo-deny` 0.20.2 for RustSec advisories, SPDX license choices,
source provenance, and dependency bans. Its configuration is the root
`deny.toml`. Include development and build dependencies in license review,
reject unknown registries and Git sources, reject yanked packages and wildcard
registry requirements, and treat duplicate transitive versions as reviewable
warnings. Exclude unpublished workspace crates from third-party license checks
and local path dependencies from workspace declaration deduplication; the
architecture checker owns those edges. Do not add blanket advisory exceptions.

Use pinned `cargo-machete` 0.9.2 with Cargo metadata for unused direct
dependency detection. Treat its output as a blocking review signal, with
package-local documented exceptions only for proven false positives.

Run both commands in a separate CI job after repository and architecture
policy. Document identical local commands, tool versions, advisory-data
requirements, and exception review. Add weekly Dependabot proposals for Cargo,
GitHub Actions, and Docker inputs with bounded open PRs. Automated proposals
still require normal review and quality gates.

Pin the test-only `wat` crate to `1.254.0` so fixture encoding uses the same
wasm-tools parser and encoder generation as the currently selected WIT tooling.
Keep this a dependency and fixture compatibility change; do not change runtime
protocol behavior.

Update Wasmtime from `48.0.2` to `48.0.3` to resolve the two RustSec
advisories found when the policy was first applied. Keep the version within
the already selected `48.x` line.

## Consequences

Newly disclosed advisories, disallowed licenses or sources, yanked packages,
and unused declarations can fail before merge. The RustSec database introduces
a network dependency for the supply-chain job. CI installation of pinned tools
adds time but keeps audit tooling outside deployable application crates.

The `wat` pin narrows test fixture resolution and can remove a duplicate
transitive wasm-tools generation from the lockfile. Dependency updates should
review whether it remains aligned with the WIT tooling rather than advancing
it independently.

The checks do not prove legal compliance, maintenance quality, or runtime
reachability. Review still considers these questions and handles justified
exceptions explicitly.

## Alternatives considered

Relying only on GitHub alerts was rejected because repository CI would not
enforce the same license, source, and unused-dependency policy locally.

Making all transitive version duplication a hard error was rejected because
unrelated upstream constraints can require more than one version. Warnings
preserve visibility while direct dependency and workspace-declaration drift
remain blocking.

Adding audit tools as workspace dependencies was rejected because they belong
to the contributor and CI environment, not the server dependency graph.
