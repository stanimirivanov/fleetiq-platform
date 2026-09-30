# FleetIQ coding harness

## TL;DR

- Use the [documentation map](../README.md) to load only guidance relevant to
  the task.
- Run `cargo xtask repository` and `cargo xtask architecture` for immediate,
  database-independent structural feedback.
- Run the complete PostgreSQL-backed Rust gate before handoff; use Compose
  checks when deployment or runtime behavior changes.
- Report every unavailable check as not run with its blocker and residual
  risk. The repository does not yet claim a canonical supply-chain aggregate.
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
| T1 — structure | During editing | `cargo xtask repository` and `cargo xtask architecture`; deterministic and independent of PostgreSQL or network once the toolchain and dependencies are available. |
| T2 — Rust acceptance | Before handoff or pull request | Formatting, SQLx metadata, Clippy, workspace tests, and rustdoc against a fully migrated supported PostgreSQL database. |
| T3 — specialized | When an affected boundary requires it | Local Compose build, health, isolation, and shutdown checks; explicitly requested security, license, fuzz, performance, or platform-specific checks. |

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
| [Dependency rules](../architecture/dependency-rules.md) | Allowed workspace package classifications and edges | Crate or direct dependency changes |
| [Engineering standards](engineering-standards.md) | Rust design, tests, documentation, security, and operations | Implementation work |
| [SQL migration criteria](sql-migrations.md) | Persisted-data invariants and migration verification | Database work |
| [Operations index](../operations/README.md) | Installation and runtime procedures | Deployment or operational work |
| [ADR index](../adr/README.md) | Accepted durable decisions and supersession history | Decisions relevant to the task |
| [Milestones](../roadmap/milestones.md) | Intended outcome sequence | Planning and issue drafting |
| [Security policy](../../SECURITY.md) | Vulnerability reporting and security-sensitive change expectations | Security-sensitive work |

Guidance should state intent, boundaries, and where its claims are verified.
Keep each normative rule in one canonical document and link to it from concise
entry points.

## Feedback sensors

Contributors may run a focused test while editing, but must run every
applicable acceptance check before handoff.

| Command or sensor | What it proves | Tier and dependencies |
|:--|:--|:--|
| `cargo xtask repository` | Local Markdown links and anchors, long-document TL;DR policy, ADR and milestone indexes, and issue or pull request template structure satisfy mechanical repository policy | T1; checked-out repository and Rust toolchain |
| `cargo xtask architecture` | Workspace packages and direct internal dependencies match the reviewed capability policy | T1; Cargo metadata, no database |
| `cargo deny check` | Locked dependencies satisfy RustSec advisory, license, source, and ban policy | T2; pinned CLI and network access to the advisory database |
| `cargo machete --with-metadata --skip-target-dir` | No unused direct dependency remains in a workspace manifest | T2; pinned CLI and Cargo metadata; reported findings require review |
| `cargo fmt --all --check` | Rust formatting is stable without changing files | T2; Rust toolchain |
| `cargo sqlx prepare --check --workspace -- --all-targets --all-features` | Checked SQL matches the migrated database and committed `.sqlx` descriptions | T2; SQLx CLI and migrated PostgreSQL |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Workspace code passes the configured Rust static analysis | T2; migrated PostgreSQL for checked queries |
| `cargo test --workspace --all-features --locked` | Unit and integration behavior passes with the committed dependency graph | T2; migrated PostgreSQL for database tests |
| `cargo doc --workspace --no-deps --locked` | Public documentation builds with workspace rustdoc lints | T2; migrated PostgreSQL for checked queries |
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
  inside the repository; fragments on Markdown targets name an existing
  GitHub-style heading anchor;
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

## Extending the harness

Add guidance or a sensor only when it closes a demonstrated gap. Prefer
extending an existing canonical document or `cargo xtask` command over
creating a parallel tool. A new sensor must be deterministic and cross-platform
where the owning tier is cross-platform, use repository-pinned dependencies,
test useful failure diagnostics, and state which tier and CI job own it.

If a new language workspace or deployable component is introduced, connect
its checks to the appropriate documented tier. Contributors must not have to
discover a hidden acceptance command from CI failures.
