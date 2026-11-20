# Contributing to FleetIQ

## TL;DR

- Deliver one coherent, verified capability per issue and pull request.
- Use the [documentation map](docs/README.md) to read only the sources relevant
  to the task.
- Preserve unrelated work and make only reversible local assumptions.
- Keep dependencies pointing inward and untrusted input behind adapters.
- Run the applicable harness tier and report checks exactly as run.
- Preserve published milestone identities and apply relevant milestones.
- Update code, tests, contracts, documentation, and operations together.

## Workflow authority

The root [AGENTS.md](AGENTS.md) routes work through installed `mattpocock/skills`.
This guide owns technical verification and safety constraints, not a parallel
planning, ticketing, implementation or review process. Read
[issue tracker](docs/agents/issue-tracker.md), [triage labels](docs/agents/triage-labels.md)
and [domain docs](docs/agents/domain.md) for the selected skill's configuration.

## Policy language and sources of truth

**MUST** and **MUST NOT** are requirements. **SHOULD** and **SHOULD NOT** are
strong defaults whose exceptions need a recorded reason. **MAY** identifies an
optional choice.

| Concern | Canonical source |
|:--|:--|
| Workflow, issue shape, review, and completion | Installed `mattpocock/skills`, configured in [AGENTS.md](AGENTS.md) |
| Concise tool-facing working agreement | [AGENTS.md](AGENTS.md) |
| Progressive task-to-document routing | [documentation map](docs/README.md) |
| Harness tiers and executable feedback | [coding harness](docs/development/harness.md) |
| Executable check inventory, runner groups, and safe result evidence | [verification guide](docs/development/verification.md) |
| Rust, architecture, tests, and operations | [engineering standards](docs/development/engineering-standards.md) |
| Direct dependency ownership and workspace lint policy | [dependency rules](docs/architecture/dependency-rules.md) |
| PostgreSQL schemas and migrations | [SQL migration criteria](docs/development/sql-migrations.md) |
| Current runtime and capability inventory | [current state](docs/development/current-state.md) |
| Durable architecture choices | [ADRs](docs/adr/README.md) |
| Intended delivery sequence | [milestones](docs/roadmap/milestones.md) |

Technical sources govern their stated constraints; the root AGENTS.md routes workflows. An accepted ADR governs the decision
it records until superseded. Surface a conflict instead of quietly choosing the
convenient rule.

## Verification

Before creating a PR, run `cargo xtask verify --group supply-chain` on the final
candidate. Failed checks or missing prerequisites leave dependency review incomplete.

Run the fast, database-independent structural checks during editing:

```bash
cargo xtask repository
cargo xtask architecture
```

Before handoff, run the complete Rust acceptance gate with `DATABASE_URL`
pointing to the fully migrated supported PostgreSQL database:

```bash
cargo xtask repository
cargo xtask architecture
cargo xtask migrations --base origin/main
cargo xtask secrets --base origin/main
cargo xtask secrets --fixtures
cargo deny check
cargo machete --with-metadata --skip-target-dir
cargo fmt --all --check
cargo sqlx prepare --check --workspace -- --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features --locked --profile ci
cargo test --workspace --all-features --locked --doc
cargo doc --workspace --no-deps --locked
```

The history and secret checks require a current trusted base and the pinned
scanner. Fetch the latest remote history before selecting `origin/main`; for a
PR or pushed revision, record the exact event base SHA used by CI. Do not choose
an older base to hide already-published changes. Install the checksummed scanner
with `node scripts/install-gitleaks.mjs`. See [harness protection](docs/development/harness-protection.md)
for installation, scan scope, fixture checks, policy ownership, and failure
handling. Missing tools, unavailable bases, or scan failures cannot be skipped.

When changing the published HTTP contract, use the [OpenAPI maintenance
skill](.agents/skills/openapi/SKILL.md), then run:

```bash
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
a separate job. A prerequisite that blocks execution is **not run**. A started
command that fails, including an advisory-database fetch failure, is **failed**;
report the operational blocker and incomplete review rather than assuming a
policy finding. Neither result is a passing dependency review. See the
[verification guide](docs/development/verification.md#execution-and-result-semantics).

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
supply-chain commands above remain required; record their exact evidence.

The [authoritative inventory and runner](docs/development/verification.md)
coordinate these ordinary commands with `cargo xtask verify --group acceptance --base origin/main`; the thirteen-command gate above preserves all checks and runs doctests separately.
`cargo xtask verify --list` prints inventory metadata without running checks.
Focused architecture, supply-chain, Rust, and OpenAPI groups provide explicit
scope and fresh ignored metadata-only evidence. Passing a focused group does
not replace acceptance or additional applicable contract and Compose checks.
Prepare prerequisites explicitly; online database checks require `SQLX_OFFLINE`
to be unset. CI wiring is checked against a reviewed workflow template and the
same inventory; matching policy does not attest its independent integrity.
Action major-tag updates use the workflow as their only version source; the
[CI resilience rules](docs/development/verification.md#ci-resilience-rules)
own tooling isolation, cache inputs, and dependency-versus-policy review.

## Documentation

Commit generated output only when consumers require it and regeneration is deterministic.

Code and documentation change together when behavior, contracts,
configuration, migrations, or operations change. Long documents include a
`## TL;DR` immediately after their title and any status metadata. A document is
long when it has at least 800 words, more than five second-level sections, or is
an architecture, security, migration, operational, or end-to-end guide.
ADRs and issue or pull request templates are exempt because their fixed
headings implement their formats. Their structure is validated separately.
The two [scoped agreements](docs/development/harness.md#scoped-guidance) and the
documentation map retain visible canonical links in plain Markdown; comment,
fenced, image-only, and unused reference-definition links do not supply routes.
This narrow routing rule is checked by `cargo xtask repository`; it does not
replace review of guidance meaning or ADR approval evidence.

Checked-in issue and PR templates expose the configured skill and technical evidence
requirements. Required ADR headings,
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

Use the `mattpocock/skills` PR workflow and the repository template: Summary,
Evidence and Merge Danger. Include issue links, relevant milestone, compatibility,
security, operational and rollback effects where applicable. Keep unavailable
verification and residual risk explicit.

## Improving the harness

When the same review finding or escaped defect recurs, close the earliest
reliable feedback loop:

1. Clarify intent in the narrow canonical guide and route readers to it.
2. Add or improve a deterministic repository sensor for a structural rule.
3. Add a focused behavioral test or fixture for an observable regression.
4. When automation would be noisy, record an explicit exception, owner, and
   review trigger.

Harness policy and scanner exceptions require review by the owners in
[CODEOWNERS](.github/CODEOWNERS). Keep policy changes explicit in the PR;
prefer a dedicated harness PR when weakening or excepting an existing gate.
Agents must not add baselines, ignore files, inline suppressions, or detector
exclusions simply to make a feature pass. GitHub rulesets must require Code
Owner approval and the relevant status checks; those settings are external to
this repository and must be verified by a repository administrator.

Sensors must produce actionable diagnostics, stay deterministic, and link to
the policy they enforce. They must not silently rewrite expected content,
depend on undeclared ambient tools, weaken invariants, or treat an unavailable
check as passing. See the [coding harness](docs/development/harness.md) for the
feedback inventory and extension criteria.

## Completion report

Use the selected skill's delivery format. Report changed behavior, verification
outcomes and material limitations; distinguish passed, failed and not-run checks.
