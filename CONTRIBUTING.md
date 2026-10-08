# Contributing to FleetIQ

## TL;DR

- Deliver one coherent, verified capability per issue and pull request.
- Use the [documentation map](docs/README.md) to read only the sources relevant
  to the task.
- Preserve unrelated work and make only reversible local assumptions.
- Keep dependencies pointing inward and untrusted input behind adapters.
- Run the applicable harness tier and report checks exactly as run.
- Use milestone titles `MNN - Outcome` and include one milestone in each issue.
- Update code, tests, contracts, documentation, and operations together.

## Policy language and sources of truth

**MUST** and **MUST NOT** are requirements. **SHOULD** and **SHOULD NOT** are
strong defaults whose exceptions need a recorded reason. **MAY** identifies an
optional choice.

| Concern | Canonical source |
|:--|:--|
| Workflow, issue shape, review, and completion | This document |
| Concise tool-facing working agreement | [AGENTS.md](AGENTS.md) |
| Progressive task-to-document routing | [documentation map](docs/README.md) |
| Harness tiers and executable feedback | [coding harness](docs/development/harness.md) |
| Rust, architecture, tests, and operations | [engineering standards](docs/development/engineering-standards.md) |
| Direct workspace dependency policy | [dependency rules](docs/architecture/dependency-rules.md) |
| PostgreSQL schemas and migrations | [SQL migration criteria](docs/development/sql-migrations.md) |
| Current runtime and capability inventory | [current state](docs/development/current-state.md) |
| Durable architecture choices | [ADRs](docs/adr/README.md) |
| Intended delivery sequence | [milestones](docs/roadmap/milestones.md) |

The narrower source governs its concern. An accepted ADR governs the decision
it records until superseded. Surface a conflict instead of quietly choosing the
convenient rule.

## Before starting

A contributor must:

- inspect the branch and working tree and preserve unrelated changes;
- use the [documentation map](docs/README.md) to read the relevant product,
  architecture, development, operations, and accepted-decision documents;
- define the smallest independently reviewable outcome;
- identify affected contracts, migrations, security boundaries, documentation,
  and operational behavior; and
- select one milestone from the roadmap.

Use an ADR when a choice affects compatibility, persisted meaning, security,
deployment topology, foundational technology, ownership, or several crates.
Local implementation choices do not need an ADR.

## Ambiguity and assumptions

Inspect code, tests, documentation, ADRs, fixtures, and history before asking
for a decision. A documented assumption is acceptable when it is local,
reversible, inexpensive, and does not alter a public contract, persisted
meaning, security boundary, external state, or acceptance criterion.

Request a decision when missing information could materially change behavior,
scope, compatibility, data meaning, security, an irreversible action, external
state, cost, or deployment topology. State the decision, evidence, options, and
consequences.

## Issues and milestones

Issue-first work is preferred. When implementation starts without an issue,
finish the smallest coherent scope and provide proposed issue text afterward.
Creating an issue, milestone, or pull request is an external action and is done
only when authorized.

Milestones use `MNN - Short outcome`, beginning at `M01`. Published numbers are
not reused or renumbered. Every implementation issue names exactly one
milestone from [the roadmap](docs/roadmap/milestones.md).

Use an imperative title and this body:

```markdown
**Milestone:** MNN - Outcome

## Goal

Describe the problem and observable result.

## Scope

- Included behavior and boundaries.

## Design decisions

- Important choices, assumptions, compatibility effects, and ADR links.

## Acceptance criteria

- [ ] Observable behavior and verification evidence.
- [ ] Relevant negative and failure behavior.
- [ ] Documentation and operational effects.

## Out of scope

- Explicit exclusions and deferred work.
```

Acceptance criteria describe observable behavior or verifiable invariants.
Record known limitations explicitly.

The checked-in implementation issue template also owns its canonical GitHub
front matter: `Implementation task`, its reviewable-capability description, and
empty default title, labels, and assignees. Change that metadata and the
repository checker together when the project intentionally changes issue
routing.

## Pull-request-sized work

A pull request must solve one problem or deliver one coherent vertical
capability. It includes the code, tests, documentation, contract changes, and
migration required to prove that capability while keeping the repository
buildable.

Split independent behavior, broad cleanup, dependency upgrades, schema
redesign, unrelated formatting, and separate architecture decisions. Do not
add placeholder abstractions, empty modules, or speculative dependencies.

## Development workflow

1. Select or define the issue and milestone.
2. Create a short issue-oriented branch.
3. Add or update tests with observable behavior.
4. Implement the smallest coherent solution.
5. Format and run the exact quality gate.
6. Review the complete diff for secrets, generated churn, accidental contract
   changes, assumptions, and unrelated edits.
7. Update documentation, ADRs, migrations, and operational guidance.
8. Open a linked pull request when authorized.

Commit subjects should be imperative and describe the resulting behavior.
Generated output is committed only when consumers require it and regeneration
is deterministic.

## Verification

Run the fast, database-independent structural checks during editing:

```powershell
cargo xtask repository
cargo xtask architecture
```

Before handoff, run the complete Rust acceptance gate with `DATABASE_URL`
pointing to the fully migrated supported PostgreSQL database:

```powershell
cargo xtask repository
cargo xtask architecture
cargo deny check
cargo machete --with-metadata --skip-target-dir
cargo fmt --all --check
cargo sqlx prepare --check --workspace -- --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features --locked
cargo doc --workspace --no-deps --locked
```

When changing the published HTTP contract, use the [OpenAPI maintenance
skill](.agents/skills/openapi/SKILL.md), then run:

```powershell
pnpm install --frozen-lockfile
pnpm lint:openapi
pnpm bundle:openapi
pnpm check:openapi
```

Commit the regenerated standalone bundle. The CI OpenAPI job checks the modular
source and confirms that the committed bundle is byte-for-byte current.

CI applies the full migration chain before compiling SQLx queries. A skipped
database-backed test is not passing evidence. Narrow crate or test commands are
useful during development but do not replace the applicable workspace gate.
The repository and architecture checks require neither PostgreSQL nor network
access after the pinned Rust toolchain and dependencies are available. Run
them before database-backed checks.

The supply-chain checks require the pinned `cargo-deny` and `cargo-machete`
versions from the [dependency management guide](docs/development/dependency-management.md).
`cargo deny check` fetches the current RustSec advisory database. Run both
checks on the locked workspace before the database-backed gate; CI runs them in
a separate job. A tool download or advisory-database failure is **not run**,
not a passing dependency review.

Regenerate `.sqlx` with the same workspace, target, and feature arguments after
changing a migration or checked query. The committed descriptions let image
builders compile without database access; online CI checking remains the
authority for schema compatibility.

If a required check cannot run, report the exact check as **not run**, the
blocking condition, checks that did run, and the residual risk. Never weaken a
check to suit the current environment or report an unexecuted check as passed.

The [coding harness](docs/development/harness.md) classifies these checks from
T0 task routing through T3 service-backed and explicitly requested specialist
checks. Local Compose smoke verification is required when deployment,
configuration, startup, readiness, or container behavior changes. The
repository does not yet define a canonical supply-chain aggregate; record any
explicitly requested vulnerability or license command exactly rather than
implying an undocumented gate exists.

## Documentation

Code and documentation change together when behavior, contracts,
configuration, migrations, or operations change. Long documents include a
`## TL;DR` immediately after their title and any status metadata. A document is
long when it has at least 800 words, more than five second-level sections, or is
an architecture, security, migration, operational, or end-to-end guide.
ADRs and issue or pull request templates are exempt because their fixed
headings implement their formats. Their structure is validated separately.
Mechanically checked issue and pull-request prompts, required ADR headings,
lifecycle metadata and index entries, and milestone indexes and boundaries use
plain Markdown. Raw HTML comments may provide author guidance, but raw HTML
elements must not carry required policy fields.

Public Rust APIs document purpose, invariants, errors, and examples where an
example clarifies correct use. Accepted ADRs are historical records; supersede
rather than rewrite them.

A crate README is required only when the crate has independent setup,
configuration, operation, publication, or an external contributor workflow.
Otherwise crate-level `//!` rustdoc is the canonical overview and the root
repository map provides navigation.

## Pull request description

A pull request description must state the linked issue and exact milestone;
the problem and resulting behavior; scope and deliberate exclusions;
assumptions, limitations, and unresolved questions; design, architecture, and
compatibility effects; migration, rollout, rollback, security, and operational
effects; and verification evidence. Every applicable check must be shown as
passed, failed, or not run. A not-run check includes the blocking condition and
residual risk.

Use [the repository template](.github/PULL_REQUEST_TEMPLATE.md). The template
collects this evidence but does not replace the policies in this document.
The checked-in template owns its exact level-two section order and required
prompt vocabulary. Change that contract and the repository checker together
when the project intentionally changes review routing. Keep every verification
command as one exact inline-code value in the first table column, and retain the
ordered unchecked review checklist; these shapes make drift mechanically
detectable.

When a dependency changes, explain the required capability and owning layer,
alternatives, enabled features, MSRV, license and source, maintenance and
security status, dependency-tree change, and exit path in the template's
dependency-review prompt.

## Improving the harness

When the same review finding or escaped defect recurs, close the earliest
reliable feedback loop:

1. Clarify intent in the narrow canonical guide and route readers to it.
2. Add or improve a deterministic repository sensor for a structural rule.
3. Add a focused behavioral test or fixture for an observable regression.
4. When automation would be noisy, record an explicit exception, owner, and
   review trigger.

Sensors must produce actionable diagnostics, stay deterministic, and link to
the policy they enforce. They must not silently rewrite expected content,
depend on undeclared ambient tools, weaken invariants, or treat an unavailable
check as passing. See the [coding harness](docs/development/harness.md) for the
feedback inventory and extension criteria.

## Review checklist

- [ ] The issue names one milestone and one coherent capability.
- [ ] Repository policy and architecture checks pass.
- [ ] Dependencies point inward and infrastructure stays outside domain policy.
- [ ] Public and persisted contracts are compatible or have an evolution plan.
- [ ] Tests cover success and relevant invalid, failure, and concurrency paths.
- [ ] Tenant isolation, idempotency, retries, and resource bounds are explicit.
- [ ] Rust, SQL, documentation, and operational guidance are current.
- [ ] Required checks passed or unavailable checks are reported honestly.
- [ ] No secrets, customer data, generated noise, or unrelated changes exist.
- [ ] An ADR is present when the choice meets the ADR threshold.

## Completion report

After a completed coding task, report:

1. the repository and exact milestone;
2. a proposed GitHub issue title;
3. a copy/paste-ready issue body using the required structure;
4. assumptions, limitations, and unresolved questions; and
5. verification commands and outcomes under **passed**, **failed**, and
   **not run**.

This report does not imply that an external issue or pull request was created.
