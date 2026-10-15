# FleetIQ coding harness

## TL;DR

- Use the [documentation map](../README.md) to load only guidance relevant to
  the task.
- Run `cargo xtask repository` and `cargo xtask architecture` for immediate,
  database-independent structural feedback.
- Protect published migrations and scan reviewable files and incoming commits
  using the [trusted-base checks](harness-protection.md).
- Run `pnpm check:openapi` for changes to the published HTTP contract.
- Run the complete PostgreSQL-backed Rust gate before handoff; use Compose
  checks when deployment or runtime behavior changes.
- Use the [verification inventory and runner](verification.md) for explicit group
  scope and safe evidence; a focused group never replaces acceptance.
- Report blocked execution as not run and an unsuccessful started command as
  failed, with its blocker and residual risk.
- Turn recurring review findings into clearer guidance, an early deterministic
  sensor, a behavioral test, or an explicit owned exception.

## Purpose

The coding harness makes the repository understandable and self-checking for
human contributors and coding agents. Feed-forward guidance explains intent
and constraints before editing. Feedback sensors detect structural violations
and behavioral regressions after each change. The shortest useful loop runs
cheap checks first while preserving one documented acceptance gate.

This guide inventories the harness. It does not replace the canonical workflow
in [CONTRIBUTING.md](../../CONTRIBUTING.md), the engineering rules in
[engineering standards](engineering-standards.md), or the exact CI workflow in
[`.github/workflows/ci.yml`](../../.github/workflows/ci.yml).

## Cost and timing tiers

| Tier | Intended timing | Current FleetIQ feedback |
|:--|:--|:--|
| T0 — route | Before editing | Read-only task routing through [the documentation map](../README.md), then relevant architecture and ADR sources. |
| T1 — structure | During editing | `cargo xtask repository` and `cargo xtask architecture`; plus trusted-base migration and secret checks; no PostgreSQL, with Git history and the pinned scanner required for protection checks. |
| T2 — Rust and supply-chain acceptance | Before handoff or pull request | Pinned supply-chain checks, formatting, SQLx metadata, Clippy, workspace tests, and rustdoc; database checks require fully migrated supported PostgreSQL. |
| T3 — specialized | When an affected boundary requires it | Local Compose build, health, isolation, and shutdown checks; additional explicitly requested fuzz, performance, security, or platform-specific checks. |

A tier describes cost and timing, not permission to skip an applicable check.
Follow the constrained-environment reporting rules in
[CONTRIBUTING.md](../../CONTRIBUTING.md#verification) whenever a required check
cannot run.

## Feed-forward guidance

| Guide | What it supplies | Load when |
|:--|:--|:--|
| [AGENTS.md](../../AGENTS.md) | Concise tool-facing agreement and canonical links | Every task |
| [CONTRIBUTING.md](../../CONTRIBUTING.md) | Workflow, issue shape, verification, review, and completion policy | Every task |
| [Documentation map](../README.md) | Progressive task-to-source routing | Every task |
| [Product scope](../product/README.md) | Users, vocabulary, outcomes, and non-goals | Product behavior or terminology changes |
| [Architecture overview](../architecture/README.md) | System boundaries, information flow, trust, and ownership | Code or boundary changes |
| [Dependency rules](../architecture/dependency-rules.md) | Package roles, exact internal/external ownership, and shared lint inheritance | Crate or direct dependency changes |
| [Engineering standards](engineering-standards.md) | Rust design, tests, documentation, security, and operations | Implementation work |
| [Harness protection](harness-protection.md) | Trusted bases, immutable SQL history, secret detector scope, exceptions, and policy ownership | History, scanner, or policy changes |
| [Reproducible verification](verification.md) | Authoritative command inventory, explicit runner groups, prerequisites, safe evidence, and reviewed CI generation | Verification, CI, or result-reporting changes |
| [SQL migration criteria](sql-migrations.md) | Persisted-data invariants and migration verification | Database work |
| [OpenAPI maintenance skill](../../.agents/skills/openapi/SKILL.md) | Modular HTTP contract layout, references, registration, and bundle workflow | Published HTTP contract work |
| [Operations index](../operations/README.md) | Installation and runtime procedures | Deployment or operational work |
| [ADR index](../adr/README.md) | Accepted durable decisions and supersession history | Decisions relevant to the task |
| [Milestones](../roadmap/milestones.md) | Intended outcome sequence | Planning and issue drafting |
| [Security policy](../../SECURITY.md) | Vulnerability reporting and security-sensitive change expectations | Security-sensitive work |

Guidance should state intent, boundaries, and where its claims are verified.
Keep each normative rule in one canonical document and link to it from concise
entry points.

## Scoped guidance

Two directory agreements serve demonstrated recurring tasks:
[repository sensor work](../../crates/xtask/AGENTS.md), exercised in PRs #263,
#265, and #267, and [modular HTTP contract work](../../contracts/http/AGENTS.md).
They route to existing canonical guidance instead of copying the full working
agreement into every crate. Root policy still applies. Add another scope only
when repeated work exposes a distinct context or workflow; change the routing
sensor and its fixtures with that deliberate policy addition.

`cargo xtask repository` requires both scoped files and their canonical rendered
links, plus routes from the documentation map. Those three route sources use
plain Markdown: non-comment raw HTML is rejected to avoid guessing whether an
HTML container hides a link. Author comments are permitted but do not satisfy a
route. Fences, images, empty links, and unused reference definitions cannot
stand in for a visible link. Other documents retain their existing HTML policy.

The sensor checks presence and navigation. Semantic accuracy, task suitability,
ADR approval, and actual agent permissions remain review concerns; a guide
cannot grant tools or certify its own correctness.

## Feedback sensors

Contributors may run a focused test while editing, but must run every
applicable acceptance check before handoff. The following table explains sensor
purpose; the [executable catalogue](../../crates/xtask/src/verification/catalogue.rs)
is authoritative for command arguments, grouping, versions, and prerequisites.
The [verification guide](verification.md) defines runner scope and honest
metadata-only evidence.

| Command or sensor | What it proves | Tier and dependencies |
|:--|:--|:--|
| `cargo xtask repository` | Local Markdown links and anchors, long-document TL;DR policy, ADR and milestone indexes, and issue or pull request template structure satisfy mechanical repository policy | T1; checked-out repository and Rust toolchain |
| `cargo xtask architecture` | Workspace roles, direct internal/external dependencies, and lint inheritance match exact policy; nonmember paths and root source overrides are rejected; the approved OIDC/RSA exception retains its exact locked graph | T1; Cargo metadata and lockfile, no database |
| `cargo xtask migrations --base origin/main` | Published SQL blobs match the working tree and index; additions have unique forward versions | T1; explicit trusted ancestor base and Git history, no database |
| `cargo xtask secrets --base origin/main` | Reviewable files, staged content, and incoming commits satisfy current and available published detector policies | T1; Git history and pinned Gitleaks, no database |
| `cargo xtask secrets --fixtures` | Real pinned detector rejects tested suppression bypasses and detects staged and removed incoming secrets | T1; Git and pinned Gitleaks, runs in early CI |
| `cargo test -p fleetiq-xtask --locked` | Repository sensors reject tested bypasses and malformed inputs as well as accepting valid policy | T1 when changing sensors; Linux architecture and Windows harness jobs; no database |
| `pnpm check:openapi` | Parsed modular OpenAPI policy, positive and bypass fixtures, and committed bundle freshness | T1 for HTTP contract changes; locked tools; Linux OpenAPI and Windows harness jobs, no database |
| `cargo deny check` | Locked dependencies satisfy RustSec advisory, license, source, and ban policy | T2; pinned CLI and network access to the advisory database |
| `cargo machete --with-metadata --skip-target-dir` | No unused direct dependency remains in a workspace manifest | T2; pinned CLI and Cargo metadata; reported findings require review |
| `cargo fmt --all --check` | Rust formatting is stable without changing files | T2; Rust toolchain |
| `cargo sqlx prepare --check --workspace -- --all-targets --all-features` | Checked SQL matches the migrated database and committed `.sqlx` descriptions | T2; SQLx CLI and migrated PostgreSQL |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Workspace code passes the configured Rust static analysis | T2; migrated PostgreSQL for checked queries |
| `cargo test --workspace --all-features --locked` | Unit and integration behavior passes with the committed dependency graph | T2; migrated PostgreSQL for database tests |
| `cargo doc --workspace --no-deps --locked` | Public documentation builds with workspace rustdoc lints | T2; migrated PostgreSQL for checked queries |
| `cargo xtask verify --check-ci` | Workflow structure and inventoried commands/tools match the template; workflow-owned action SHA pins satisfy repository, format, and consistency rules | T1; repository policy files; also performed before each runner group |
| `cargo xtask verify --group acceptance --base origin/main` | Coordinates every canonical acceptance command and records selected-scope results without treating blocked execution as success | T1/T2; all inventory prerequisites; focused groups have smaller explicit scope |
| Local Compose smoke checks | The container builds, starts, reports health, preserves isolation, and stops cleanly | T3; Docker with Compose and free configured host ports |

The T2 commands, order, and unavailable-check protocol are canonical in
[CONTRIBUTING.md](../../CONTRIBUTING.md#verification). CI runs T1 before the
separate supply-chain, database-backed Rust, and local Compose jobs. The
[dependency management guide](dependency-management.md) records tool versions,
exceptions, and update review.

## Repository policy

`cargo xtask repository` discovers policy-controlled Markdown files below the
repository root and reports all violations in stable path and line order. It
checks these mechanical rules without network access:

- repository-local Markdown destinations use exact path capitalization and stay
  inside the repository; machine-specific absolute paths are rejected before
  and after percent decoding, and fragments on Markdown targets name an existing
  GitHub-style heading anchor;
- the two task-scoped agreements and documentation map retain the canonical
  visible routes described in [scoped guidance](#scoped-guidance);
- a long document begins its level-two sections with `## TL;DR` under the
  criteria in [CONTRIBUTING.md](../../CONTRIBUTING.md#documentation); ADRs and
  GitHub issue or pull request templates remain exempt because their headings
  implement fixed formats;
- ADR files use zero-padded numbers and lowercase kebab-case names, match their
  heading number, contain the required sections, form a contiguous sequence,
  retain the published high-water mark, appear exactly once in the index, and
  use valid lifecycle declarations when present;
- milestone index entries use contiguous `MNN - Outcome` identifiers and match
  their boundary headings and published high-water mark; and
- the issue and pull request templates retain the milestone field, sections,
  exact first-column verification commands, and ordered review checklist
  required by [CONTRIBUTING.md](../../CONTRIBUTING.md).

Mechanically checked template prompts, required ADR headings, lifecycle
metadata and index entries, and milestone indexes and boundaries use plain
Markdown. Raw HTML comments may provide author-only guidance; required fields
cannot be carried by raw HTML elements.

The check ignores external URL availability and links expressed only through
raw HTML. Those need semantic review or a future advisory sensor. A passing
repository check proves document structure and local navigation, not factual
accuracy or freshness.

The supply-chain job uses the repository-owned `deny.toml` and the same pinned
CLI versions documented for local runs. Advisory data changes between runs, so
an unavailable database or new finding is reported explicitly and never
treated as a pass.

## Steering loop

Classify a recurring review finding or escaped defect and close the cheapest
reliable loop:

1. If intent is unclear, improve the narrow canonical guide and its route.
2. If a structural rule is deterministic, add or improve an actionable T1
   sensor.
3. If observable behavior regressed, add a focused test or fixture at the
   lowest convincing boundary.
4. If automation would be noisy or misleading, record the exception, owner,
   reason, and review trigger.

A sensor failure should identify the violated rule, affected location,
accepted shape, likely correction, and canonical guidance. Sensors must not
rewrite expected behavior, weaken an invariant, depend on ambient global
tools, or report an unavailable check as passed.

Review harness changes like product changes. Watch false positives, repeated
suppressions, runtime, flaky retries, stale routes, and findings that continue
to escape. Promote a sensor to a blocking gate only when it is deterministic,
actionable, and owned.

## Multi-PR execution plans

Use a versioned execution plan when one approved outcome needs several
dependent pull requests, especially for migrations, compatibility transitions,
or architectural refactoring. Add the plan under `docs/development/plans/`
with the first implementation slice; do not create an empty plans directory.

The plan should record the outcome, invariants, exclusions, relevant ADRs,
ordered slices, compatibility, rollout and rollback, verification, and any
discovery that changes the sequence. Each slice remains independently
reviewable and leaves the repository buildable. GitHub issues and milestones
remain authoritative for delivery state, and ADRs remain authoritative for
durable decisions.

The approved [harness hardening plan](plans/harness-hardening.md) records the
remaining gaps and delivery order. Implemented dependency ownership and lint
inheritance, migration immutability, and pinned secret scanning are blocking
sensors. See [harness protection](harness-protection.md) for explicit trusted-base
selection, bootstrap scope, and external review requirements. Reproducible
verification coordination is implemented and locally verified; [its guide](verification.md) preserves the exact acceptance gate.
Scoped guidance, evidence-based ADR lifecycle reconciliation, and additional
OpenAPI and cross-platform bypass fixtures complete the M01 implementation.
Runtime recovery evidence and measured performance qualification remain M08 work.
External merge controls still require administrator verification.

## Extending the harness

Add guidance or a sensor only when it closes a demonstrated gap. Prefer
extending an existing canonical document or `cargo xtask` command over
creating a parallel tool. A new sensor must be deterministic and cross-platform
where the owning tier is cross-platform, use repository-pinned dependencies,
test useful failure diagnostics, and state which tier and CI job own it.

If a new language workspace or deployable component is introduced, connect
its checks to the appropriate documented tier. Contributors must not have to
discover a hidden acceptance command from CI failures.
