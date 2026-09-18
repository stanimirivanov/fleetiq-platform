# Contributing to FleetIQ

## TL;DR

- Deliver one coherent, verified capability per issue and pull request.
- Preserve unrelated work and make only reversible local assumptions.
- Keep dependencies pointing inward and untrusted input behind adapters.
- Run the complete Rust quality gate and report checks exactly as run.
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
| Rust, architecture, tests, and operations | [engineering standards](docs/development/engineering-standards.md) |
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
- read the relevant product, architecture, development, and ADR documents;
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

With `DATABASE_URL` pointing to the fully migrated supported PostgreSQL
database, run:

```powershell
cargo fmt --all --check
cargo sqlx prepare --check --workspace -- --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features --locked
cargo doc --workspace --no-deps --locked
```

CI applies the full migration chain before compiling SQLx queries. A skipped
database-backed test is not passing evidence. Narrow crate or test commands are
useful during development but do not replace the applicable workspace gate.

Regenerate `.sqlx` with the same workspace, target, and feature arguments after
changing a migration or checked query. The committed descriptions let image
builders compile without database access; online CI checking remains the
authority for schema compatibility.

If a required check cannot run, report the exact check as **not run**, the
blocking condition, checks that did run, and the residual risk. Never weaken a
check to suit the current environment or report an unexecuted check as passed.

## Documentation

Code and documentation change together when behavior, contracts,
configuration, migrations, or operations change. Long documents include a
`## TL;DR` immediately after their title and any status metadata. A document is
long when it has at least 800 words, more than five second-level sections, or is
an architecture, security, migration, operational, or end-to-end guide.
Templates are exempt because their headings are fields to complete.

Public Rust APIs document purpose, invariants, errors, and examples where an
example clarifies correct use. Accepted ADRs are historical records; supersede
rather than rewrite them.

A crate README is required only when the crate has independent setup,
configuration, operation, publication, or an external contributor workflow.
Otherwise crate-level `//!` rustdoc is the canonical overview and the root
repository map provides navigation.

## Review checklist

- [ ] The issue names one milestone and one coherent capability.
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
