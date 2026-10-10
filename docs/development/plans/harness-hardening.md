# Harness hardening execution plan

## TL;DR

Status: active. Close the audited gaps in three stages: dependency ownership,
published-history and secret protection, then reproducible verification and
recovery evidence. Deliver independently reviewable pull requests. The first
two slices enforce dependency ownership, lint inheritance, migration
immutability, and pinned secret detection. Verification coordination and recovery
remain planned; external merge protection remains unverified.

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
| 2. Published history and sensitive artifacts | Base-revision migration immutability sensor, pinned redacted secret scanning, explicit handling of policy changes and scanner exceptions | M01 - Decisions and Contracts | Implemented; local verification recorded below; external merge settings unverified |
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

Add `cargo xtask migrations --base REV` with byte comparisons against a trusted
published base, the index, and working files. Fail closed on missing/invalid
history and malformed inventories; allow only unique forward additions. Add
`cargo xtask secrets [--base REV]` with checksum-verified Gitleaks 8.30.1,
reviewable-file snapshots, staged scans, incoming-history scans, exact synthetic
exceptions applied privately by the wrapper, and metadata-only diagnostics.
Real fixtures exposed upstream global-allowlist and fingerprint-ignore bypasses;
default-only detection, exact in-memory equality, raw index snapshots, and bare
history scans now protect these boundaries. Run the real detector bypass suite
in the early CI job alongside xtask regression tests. See
[ADR 0097](../../adr/0097-protect-published-history-and-sensitive-artifacts.md)
and the canonical [harness protection guide](../harness-protection.md).

CI uses full history and event-owned base selection. Current and trusted-base
scanner policy both apply when published policy exists. The initial addition
bootstraps scanning of current/index/incoming content and does not claim all
legacy history is clean. CODEOWNERS routes policy review; required owner
approval, stale-approval dismissal, and required GitHub status checks need
administrator verification. These external settings could not be verified and
must not be reported as enforced by a file or local sensor.

Acceptance: reject published content changes/deletions/renames, staged changes
hidden by restored working files, invalid versions or bases, policy widening,
unreadable inputs, ignored fingerprints/inline suppressions, and novel values
inside an exactly excepted fixture file. Prove incoming history detects a secret
subsequently removed. Preserve complete Rust/database and supply-chain gates.

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

Local stage-2 evidence on 10 October 2026: 159 Windows xtask tests passed,
including 25 migration and 15 secret-policy cases. Three additional Unix-only
symlink/filename regressions require CI. The real checksum-verified Gitleaks
8.30.1 detector passed eight bypass scenarios. The actual repository scan passed
for 738 reviewable working files plus raw index and incoming history; the
published-history check verified 28 immutable migrations and zero additions
against `a1e09f0685abd6add53752850c14afdb22c86052` (`origin/main`). The local
incoming range is empty because the implementation is not committed yet; the
positive history fixtures prove removed incoming secrets are detected.

Repository policy passed for 139 Markdown documents; architecture policy passed
for 16 members, 41 internal and 34 external edges, and 16 inherited lint policies.
Formatting, focused all-target xtask Clippy with warnings denied, focused rustdoc,
installer syntax, cargo-deny 0.20.2, and cargo-machete 0.9.2 passed. Existing
duplicate-version supply-chain warnings remain advisory. Final independent
read-only review found no remaining actionable issue. serde_json adds one direct
tooling edge and one lockfile reference, with no new versions or production edge.
The PostgreSQL-backed workspace gate remains not run for the prerequisites
reported above. Required external merge settings remain unverified, and the
single routed owner needs an independent authorized owner/team before mandatory
Code Owner review can cover that owner's PRs.

Module cohesion, broad security completeness, model capability reduction,
customer-specific behavior, and production service objectives remain review or
later qualification concerns. GitHub issues own live delivery state; this file
records the approved sequence and discoveries.