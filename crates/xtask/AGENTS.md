# Repository sensor work

## TL;DR

This crate owns development sensors and verification coordination. Start with
the routes below, then read only the module and policy that the task changes.

## Read before editing

- [Contributor workflow and acceptance](../../CONTRIBUTING.md)
- [Engineering standards](../../docs/development/engineering-standards.md)
- [Harness tiers and sensor criteria](../../docs/development/harness.md)
- [Dependency ownership](../../docs/architecture/dependency-rules.md)
- [History, secrets, and policy review](../../docs/development/harness-protection.md)
- [Command inventory, CI, and evidence](../../docs/development/verification.md)

## Local workflow

Choose the existing owning module: architecture, repository, migrations,
secrets, or verification. Keep policy, parsing, process execution, and evidence
responsibilities explicit. Extend the current sensor before introducing a
parallel command or a second policy authority.

For a structural defect, prove the bypass at the actual Markdown, filesystem,
Git, manifest, or process boundary. Pair rejection cases with valid inputs and
assert useful diagnostics. Exercise Windows junctions and Unix symlinks where
they represent different supported filesystem behavior; fixture setup failures
are failures, never silent passes.

Preserve the canonical acceptance order, fixed command arguments, honest
failed/not-run states, metadata-only evidence, and explicit CI regeneration.
Policy changes require review; an incoming sensor cannot certify its own
integrity. Documentation does not change actual tool permissions.

## Verify from the repository root

Run `cargo test -p fleetiq-xtask --locked`, `cargo xtask repository`, and
`cargo xtask architecture`. For CI structure or tool-version changes, explicitly
regenerate with `cargo xtask verify --write-ci` and check with `--check-ci`.
Action-only SHA updates change the workflow alone and require `--check-ci`, not
a duplicate template pin or regeneration. Follow the verification guide's CI
resilience rules for provenance review and tooling isolation. Select relevant
groups from the verification guide and run the applicable contributor gate.
Report platform-specific checks and missing prerequisites exactly.
