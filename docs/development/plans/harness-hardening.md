# Harness hardening execution plan

## TL;DR

The four M01 implementation slices (stages 1, 2, 3a, and 3b) are complete with
local evidence below. They enforce dependency ownership, published history and
secret protection, verification coordination, scoped guidance, and resilient
contract and cross-platform sensors. PostgreSQL-backed acceptance, Linux/Windows
CI runs, and external merge protection remain unverified. Runtime recovery
qualification
(stage 3c) remains a separate planned M08 slice.

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
| 3a. Verification inventory and evidence | Authoritative commands and versions, explicit groups, honest metadata-only reports, and reviewed CI generation without weakening acceptance | M01 - Decisions and Contracts | Implemented; local evidence below; ADR 0098 accepted after merged PR #267; PostgreSQL gate still requires CI |
| 3b. Scoped context and sensor resilience | Task-justified scoped guidance, evidence-based ADR lifecycle reconciliation, OpenAPI bypass fixtures, and cross-platform sensor coverage | M01 - Decisions and Contracts | Implemented; local evidence below; Linux/Windows CI and external merge settings await verification |
| 3c. Runtime recovery qualification | Service outage/recovery smoke evidence, measured load baselines, and later calibrated performance budgets | M08 - Production Readiness | Planned; baseline measurement precedes blocking thresholds |

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

### Stage 3a: verification inventory and evidence

Implement one Rust-owned catalogue within the existing xtask tooling crate:
fixed command arguments, explicit groups, expected outcomes, tool versions,
prerequisites, and acceptance membership. Keep the twelve canonical acceptance
commands and their order intact. Add `cargo xtask verify` to coordinate ordinary
commands and write fresh ignored metadata-only evidence. Missing prerequisites
and process-start failures are not run; unsuccessful started commands are failed,
including operational failures whose underlying cause needs diagnosis. Neither
can become a successful review. Continue independent checks without retries.

Compare contributor and PR-template acceptance commands with the catalogue.
Generate CI from a reviewed template plus catalogue-owned group and version
placeholders, then compare the entire workflow during verification. Ordinary
checks never silently regenerate stale policy. CODEOWNERS and external review
protection remain necessary because both the inventory and template are mutable
policy. Record safe versions, explicit group scope, trusted base, source HEAD and
dirty-state observations, elapsed time, and result; persist no raw tool output,
connection URL, credential, or environment dump. Source observations are context,
not content hashes or certificates.

The [verification guide](../verification.md) documents setup, scope, failures,
privacy, and limits. [ADR 0098](../../adr/0098-coordinate-reproducible-verification-and-evidence.md)
was proposed at implementation handoff and was accepted following the approved
plan and reviewed merge of PR #267 on 10 October 2026. Passing tests alone do not
accept a durable decision. This slice changes no runtime contract or production behavior.

Acceptance: complete inventory and unchanged acceptance order; positive and
negative CI/template drift fixtures; missing/wrong tools, missing database
configuration, offline-mode refusal, command failure/start failure, continued
independent execution, fresh evidence, and redirected-path rejection. Run the
actual focused groups and applicable full gate; record results after verification.

### Stage 3b: scoped context and sensor resilience

Add scoped guidance only where repeated tasks justify it. The xtask and HTTP
contract agreements route to canonical policy; a repository sensor requires their
visible Markdown routes and rejects hidden substitutes. Presence and navigation
are deterministic; accuracy, task suitability, and permissions remain review
concerns. Extend portable Markdown checks for encoded machine paths and prove
redirect rejection using actual Windows junctions and Unix symlinks.

Use Redocly's existing public parsed-document API for modular OpenAPI ownership,
reference placement, parameter extraction, and expanded schema budgets. Pair
negative bypass fixtures with valid YAML and opaque example payloads; JSON data
must not be mistaken for contract structure. Preserve the two exact reviewed
OAuth suppressions and fail unsuppressed on new ignore entries. Bundle checks
clean temporary output on success and failure without rewriting committed bytes.

Run the existing inventoried repository, architecture, sensor-test, and OpenAPI
commands in an additional Windows CI job. Keep Linux, PostgreSQL, supply-chain,
and Compose gates and the canonical acceptance membership unchanged. CI
comparison rejects removed or replaced Windows coverage. The added status must
still be reviewed by an administrator when configuring required merge checks.

Reconcile ADRs 0068-0078 and 0098 using the approved sequence and their exact
merged PRs and commit IDs. Change lifecycle metadata, preserving historical
decisions. Merge history does not establish independent reviewer approval or
external protection settings, and passing checks cannot accept an ADR.

Acceptance: scoped rules have demonstrated owning tasks and canonical routes;
lifecycle changes identify real approval/history evidence; added sensor fixtures
prove malformed and bypassing inputs fail on their supported platforms while
valid inputs remain accepted. Deliver this as a separate M01 slice; do not combine
it with runtime qualification.

### Stage 3c: runtime recovery qualification

Extend runtime smoke scenarios to service outage and recovery with bounded,
useful failure evidence. Measure stable load baselines before proposing blocking
performance thresholds. Record environment, repeatability, recovery conditions,
and observation limits. This M08 slice belongs to production-readiness
qualification rather than development-command coordination. Calibrate and review
budgets only after measurements are stable; do not invent acceptance thresholds
from unqualified local timings.

Acceptance: outage and recovery behavior is observable under controlled failures;
failure evidence is bounded and useful; repeatable baseline measurements support
any subsequently proposed performance budget.

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

Local stage-3a evidence on 10 October 2026: all 188 Windows xtask tests passed
with no failures or ignored cases. Four additional Unix-only sensor regressions
require Linux CI. Repository policy passed for 141 Markdown documents;
architecture policy retained 16 members, 41 internal and 34 external dependency
edges, and 16 inherited lint policies. The history check retained all 28
published migrations with zero additions against
bc1c64012b146a04eec38d08b5f81f62c2910477 (origin/main).
The real scanner passed eight bypass scenarios and scanned 749 reviewable files
plus raw index and incoming history. The local incoming range is empty because
this implementation remains uncommitted; history fixtures prove removed
incoming secrets are detected.

Complete workflow comparison, formatting, focused all-target xtask Clippy with
warnings denied, focused rustdoc, pinned supply-chain checks, and the actual
OpenAPI group passed. Existing duplicate-version warnings remain advisory;
the existing two Redocly suppressions are unchanged. Real runs exposed and
corrected Windows nested executable rebuild failures and Cargo launcher
metadata leaking into cargo-machete's argument parser. The compiled-task adapter,
context isolation, and version compatibility have regression coverage.

The final acceptance report recorded eight passed, zero failed, and four
not-run checks; its exit was nonzero as required for incomplete acceptance.
DATABASE_URL remains unset, so SQLx metadata, workspace Clippy, workspace tests,
and workspace rustdoc were not run. No Docker executable is exposed in this
session. CI must still verify the PostgreSQL-backed gate; Compose runtime
qualification is unchanged. No production code, migration, contract, dependency
version, or lockfile changed. Metadata reports remain ignored and do not attest
dirty-file contents, frozen advisory data, policy integrity, or external merge
controls. ADR 0098 was proposed at this handoff and subsequently accepted using
the reviewed merge of PR #267 as approval evidence.

Local stage-3b evidence on 10 October 2026: all 209 Windows xtask tests passed
with zero failures or ignored cases. The actual OpenAPI group passed 58 Node
regressions, source lint, and byte-exact bundle freshness. The two existing OAuth
suppressions remain unchanged. Live fixtures exposed and corrected false
positives for opaque JSON payload fields and a parameter-category bypass; the
supported Redocly resolver/type metadata preserves normative validators and
missing real reference failures. No parser dependency or new suppression was
added. Windows junction fixtures run on the real filesystem; Unix-only cases
still require Linux CI.

Repository policy passed for 143 Markdown documents. Architecture policy retains
16 members, 41 internal and 34 external edges, and 16 inherited lint policies.
Complete CI comparison, formatting, focused all-target xtask Clippy with warnings
denied, focused rustdoc, and the official OpenAPI skill validator passed. The
acceptance coordinator recorded eight passed, zero failed, and four not-run
checks with a nonzero exit for incomplete acceptance. Pinned supply-chain checks
and all eight real Gitleaks bypass scenarios passed; existing duplicate-version
warnings remain advisory. Current working files, raw index, and incoming history
were scanned; the incoming range remains empty while this slice is uncommitted.
The history check preserved all 28 migrations with zero additions against
`d61c887f4ddafbba0bc3cd9babbc68797404f6d9` (`origin/main`).

SQLx metadata, full workspace Clippy, tests, and rustdoc were not run because
`DATABASE_URL` is unset. These focused harness results do not replace the
PostgreSQL-backed acceptance gate. GitHub Linux/Windows execution, required
status checks, and independent Code Owner protection remain unverified; the
new Windows job alone does not make its status merge-required. No production
code, modular source schema, committed bundle, migration, dependency version,
lockfile, or canonical acceptance membership changed. ADR lifecycle updates
cite the exact merged PRs and commits while preserving historical decisions.
Reports remain ignored context metadata rather than policy or content attestations.
M01 harness implementation is complete; stage-3c runtime recovery and measured
load qualification remain planned M08 work.

Module cohesion, broad security completeness, model capability reduction,
customer-specific behavior, and production service objectives remain review or
later qualification concerns. GitHub issues own live delivery state; this file
records the approved sequence and discoveries.