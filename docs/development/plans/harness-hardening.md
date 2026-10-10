# Harness hardening execution plan

## TL;DR

Status: active. Close the audited gaps in three stages: dependency ownership,
published-history and secret protection, then reproducible verification and
recovery evidence. Deliver independently reviewable pull requests. The first
slice implements external ownership and lint inheritance; later stages remain
planned and must not be reported as enforced.

## Outcome and invariants

Shift precise repository rules from agent interpretation to early deterministic
sensors. Keep the existing progressive documentation map, exact capability
graph, typed authorization evidence, SQLx checks, and narrowly approved OIDC/RSA
exception. Preserve Rust and Cargo as the authority for language and dependency
facts. Do not add speculative runtime abstractions, blanket layer ranks, file
length gates, or mandatory CQRS ceremony.

Guides explain intent; sensors reject measurable drift. Every added sensor must
have an owner, actionable diagnostics, positive and negative fixtures, and an
explicit local and CI entry point. Unavailable checks never become passes.
Documentation cannot grant or restrict actual execution permissions: those
belong to the agent runtime and CI identity.

## Ordered delivery

| Stage | Scope | Milestone | State |
|:--|:--|:--|:--|
| 1. Dependency ownership | Exact direct external edges per member and kind, rejected nonmember paths and root source overrides, shared lint inheritance, bypass fixtures in early CI | M01 - Decisions and Contracts | Implemented; structural and harness checks verified locally |
| 2. Published history and sensitive artifacts | Base-revision migration immutability sensor, pinned redacted secret scanning, explicit handling of policy changes and scanner exceptions | M01 - Decisions and Contracts | Planned |
| 3. Verification and recovery | Reproducible check inventory and evidence, scoped context/lifecycle reconciliation, cross-platform sensor tests, runtime outage/recovery evidence and calibrated performance budgets | M01 - Decisions and Contracts for harness work; M08 - Production Readiness for runtime qualification | Planned; split into coherent PRs before implementation |

### Stage 1: dependency ownership

Extend `cargo xtask architecture` rather than add another orchestration tool.
Cargo metadata supplies canonical dependency names, kinds, paths, and inactive
declarations. Explicit per-package external policy permits current value,
adapter, composition, test, and tooling libraries. Reject unreviewed registry
edges and every nonmember path. Reject repository-root source overrides that
could bypass declaration-only metadata. Read member lint declarations with the
existing TOML parser. Keep source, license, advisory, and unused-dependency
review in the existing supply-chain gate. See [ADR 0096](../../adr/0096-enforce-external-dependency-ownership-and-lint-inheritance.md).

Acceptance: positive workspace checks plus fixtures for renamed dependencies,
optional/target declarations, test-to-runtime promotion, build dependencies,
registry homonyms, nonmember paths, source substitutions, stale/invalid policy,
and missing or replaced lint inheritance. Run the regression suite before
database jobs in CI.

### Stage 2: published history and sensitive artifacts

Compare migrations against an explicit trusted base revision. Reject changes,
renames, and deletions of published migration bytes while permitting new ordered
migrations. Cover local and pull-request base selection and missing-history
failures; do not treat lack of a base as permission to skip the check.

Add a maintained pinned secret scanner with redacted diagnostics. Define local
and CI commands, fixtures, reviewed narrow exceptions, and baseline handling.
Review policy changes independently so an agent cannot silently weaken the
sensor alongside its feature. Confirm ownership and repository branch rules;
checked-in documentation alone cannot enforce merge permissions.

### Stage 3: verification and recovery

Define one authoritative check inventory with scope, prerequisites, exact tool
versions, outputs, and passed/failed/not-run evidence. Keep the canonical Rust
gate intact and test any runner against missing prerequisites and failing
commands. Reconcile ADR lifecycle using explicit approval/history evidence;
passing tests alone do not turn a proposal into an accepted decision.

Add scoped guidance where repeated tasks justify it, cross-platform fixtures,
and OpenAPI sensor bypass tests. Extend runtime smoke scenarios to service
outage and recovery with useful bounded failure evidence. Measure stable load
baselines before proposing blocking performance thresholds. Separate harness
coordination from M08 runtime qualification so each PR proves one outcome.

## Compatibility, rollout, and rollback

These changes tighten development and CI policy without changing HTTP payloads,
persisted meaning, deployment topology, or runtime behavior. Apply each sensor
to the current valid tree before making it blocking. Treat a required policy
change as a reviewed architectural decision, not an automatic fix for a red
check. Revert a faulty sensor and its documentation together; preserve the
rule's review requirement and track the gap until corrected.

## Verification and limits

For stage 1, run `cargo xtask repository`, `cargo xtask architecture`,
`cargo test -p fleetiq-xtask --locked`, formatting, focused Clippy, and rustdoc.
Run the complete applicable acceptance gate from
[CONTRIBUTING.md](../../../CONTRIBUTING.md#verification), recording prerequisites
and results exactly. Sensor fixtures must prove useful failures, not merely
mirror implementation data. Supply-chain tools still check versions and
sources; ownership is a separate question.

Local stage-1 evidence on 10 October 2026: repository policy passed for 137
Markdown documents; architecture policy passed for 16 workspace members, 41
internal and 33 external dependency edges, and 16 inherited lint policies. All
117 xtask tests passed with the locked offline graph. Workspace formatting,
focused all-target xtask Clippy with warnings denied, focused rustdoc, cargo-deny
0.20.2, and cargo-machete 0.9.2 passed. Supply-chain duplicate-version warnings
remain advisory under existing policy. Final read-only review found no remaining
actionable issue in this slice.

The complete PostgreSQL-backed commands (`cargo sqlx prepare --check --workspace
-- --all-targets --all-features`, workspace Clippy, workspace tests, and workspace
rustdoc) were not run locally: `DATABASE_URL` is unset and no Docker executable
is exposed in this session. Focused harness checks do not substitute for that
acceptance gate; CI must still verify database compatibility and the wider
workspace. No production code, dependency graph, lockfile, or migration changed.

Module cohesion, broad security completeness, model capability reduction,
customer-specific behavior, and production service objectives remain review or
later qualification concerns. GitHub issues own live delivery state; this file
records the approved sequence and discoveries.