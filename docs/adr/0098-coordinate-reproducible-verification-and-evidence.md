# ADR 0098: Coordinate reproducible verification and evidence

## Status

Proposed, 10 October 2026.

This durable harness choice awaits human review. Authorization to implement it
and passing checks do not establish architectural acceptance; record approval
through the repository's normal review and merge process before changing this
status.

## Context

The contributor guide, pull request template, and CI repeat the same acceptance
commands. Their current structural checks preserve the template, but command
ownership, tool prerequisites, and evidence still depend on manual coordination.
Missing PostgreSQL or tool availability has repeatedly required contributors to
explain why a complete gate could not run. The approved harness hardening plan
calls for one inventory and honest, reproducible verification evidence without
weakening existing checks or introducing another orchestration framework.

## Decision

Keep contributor policy authoritative for applicability. Add an immutable Rust
check catalogue inside the existing xtask tooling crate and coordinate ordinary
commands with `cargo xtask verify`. The catalogue owns fixed executable argument
vectors, canonical display commands, groups, acceptance membership, required
tools and versions, prerequisites, and expected outcomes. Mechanical checks
compare the contributor and template acceptance lists and selected CI wiring
with that catalogue. Preserve the exact twelve-command acceptance order; focused
sensor tests and OpenAPI remain separate entries.

Require an explicit trusted ancestor base for history checks and substitute only
the resolved commit ID into fixed command arguments. Require the reviewed tool
versions, a nonempty `DATABASE_URL`, and an unset `SQLX_OFFLINE` for online
PostgreSQL acceptance. Configuration presence does not establish service health,
PostgreSQL version, migration application, or schema compatibility. The real
commands remain responsible for their results. The runner performs no setup,
installs nothing, and adds no automatic retry or inference of change scope.

Record passed, failed, and not-run results. A missing prerequisite or process
that cannot start is not run; an unsuccessfully completed or terminated command
is failed. Continue independent checks, and return a nonzero group result if any
selected check failed or could not run. Do not reinterpret every internal tool
failure as a known policy or availability condition. In particular, a started
advisory-fetch failure remains failed and needs a human-readable operational
explanation at handoff.

Create fresh ignored metadata-only JSON reports under `target/verification/`.
Record group scope, source HEAD and dirty-state observations at start and end,
trusted base, safe versions, fixed check metadata, exit status, and elapsed time.
Never persist raw process output, environment values, credentials, or connection
URLs in these reports. Ordinary tool output stays on the console. Source
observations are context rather than content hashes or certificates of trust.

Launch repository-owned checks through the current compiled xtask rather than
nested Cargo runs: Windows cannot replace a running executable during a rebuild.
Record this execution adapter and actual arguments while preserving canonical
contributor commands. Invoke other tools directly.
Use the existing Rust process APIs and maintained serde_json dependency. Add no
new runtime dependency, package, test framework, or business behavior. Route
policy review through existing CODEOWNERS. Sensors and the catalogue are mutable
reviewed policy; independent required approval and GitHub status enforcement
remain external settings that a checked-in file cannot establish.

See the [verification guide](../development/verification.md),
[catalogue](../../crates/xtask/src/verification/catalogue.rs), and
[hardening plan](../development/plans/harness-hardening.md).

## Consequences

Local and CI checks share one executable inventory while retaining ordinary
commands for troubleshooting. Missing prerequisites and failed commands produce
reviewable evidence without silent success. Focused groups accelerate feedback
without changing what acceptance requires. Metadata-only reports reduce
accidental disclosure compared with storing arbitrary tool logs.

The coordinator itself needs positive and negative fixtures, explicit pins, and
review when gate membership changes. A report cannot prove unchanged dirty-tree
content, comprehensive secret detection, database health, policy integrity, or
external merge protection. Operational failures inside started tools require
manual diagnosis. Compose recovery qualification, performance budgets, scoped
context guidance, and broader ADR lifecycle reconciliation remain later slices.

There is no change to HTTP payloads, persisted meaning, migration bytes,
production dependencies, or runtime behavior. Roll back faulty coordination and
its guide together while retaining individual checks and the canonical gate;
record the remaining drift risk rather than replacing unavailable checks with
passing claims.

## Alternatives considered

Continue copying commands in guides and CI with manual evidence. This preserves
simple tooling but leaves prerequisites and applicability easy to misreport and
creates multiple command authorities.

Adopt a new orchestration framework or external verification service. That adds
another dependency, configuration language, and trust boundary for a small set
of existing commands. The existing xtask is sufficient for the current outcome.

Automatically infer scope, install dependencies, or retry failures until green.
These choices hide configuration assumptions and can make evidence misleading.
Keep scope and setup explicit; reconsider only with a separately reviewed need
and observable failure semantics.

Persist complete stdout, stderr, or environment snapshots. Those artifacts can
leak credentials and customer data. Retain safe metadata and use existing console
diagnostics under the contributor's normal handling rules.

## Verification

Test inventory completeness and the unchanged acceptance order; missing tools,
wrong versions, unset database configuration, and offline-mode refusal; genuine
process success, failure, and start failure; continued independent execution;
no invented exit code for termination; fresh report creation and redirected-path
rejection; and drift between the catalogue, templates, contributor guide, and CI.
Run focused harness checks and the complete applicable gate, reporting unavailable
checks honestly. Passing these checks does not change this ADR's proposed status.
