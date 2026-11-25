# Repository working agreement

## TL;DR

Preserve inward
dependencies, trusted-host boundaries, tenant ownership, deterministic replay,
and immutable history. Follow the Rust and SQL standards and report every check
exactly.

This is the concise entry point for coding tools and contributors.
[`CONTRIBUTING.md`](CONTRIBUTING.md) owns technical verification. The
[documentation map](docs/README.md) routes each task to the additional sources
that apply.

## Workflow authority

Use installed `mattpocock/skills` as the primary workflow for matching tasks.
Discover and load skills on demand. When workflows overlap, use the matching
Matt Pocock skill rather than a parallel workflow. Use other skills only for
capabilities not covered by that workflow or when explicitly requested.

Repository guidance supplies technical constraints, architecture boundaries and
verification requirements. Scoped `AGENTS.md` files are subordinate for workflow
decisions and add only module-specific constraints. Explicit user instructions
and host permissions remain authoritative.

## Technical context

Read [CONTRIBUTING.md](CONTRIBUTING.md) for verification and use the
[documentation map](docs/README.md) for relevant architecture and operations.
Check [current state](docs/development/current-state.md) before assuming a planned
capability exists. For persistence, read [SQL migration criteria](docs/development/sql-migrations.md).
Use repository-pinned tools and the [harness protection guide](docs/development/harness-protection.md).
For HTTP contracts, use the [OpenAPI skill](.agents/skills/openapi/SKILL.md) and
[contract agreement](contracts/http/AGENTS.md). For sensors, read the
[xtask agreement](harness/xtask/AGENTS.md).

## Architecture and Rust

- For Cargo changes, follow the [dependency rules](docs/architecture/dependency-rules.md):
  direct dependencies require an approved owner and kind, nonmember paths are
  rejected, and every member inherits workspace lints. Run the architecture sensor.
- Dependencies point inward. Domain crates do not depend on SQLx, Wasmtime,
  transports, brokers, web frameworks, or vendors.
- Protocol decoding identifies source observations; semantic binding attributes
  them to asset properties. Preserve this boundary.
- Treat protocol packages and wire data as untrusted. The host owns transport,
  identity, resource limits, durable writes, and side effects.
- Keep domain values, wire values, SQL rows, and event envelopes distinct when
  their invariants or evolution differ.
- Model identifiers, revisions, units, time bases, and states explicitly. Make
  invalid states hard to construct.
- Put traits at the boundary that consumes them. Prefer enums, newtypes,
  composition, and exhaustive matching over class-shaped ceremony.
- Keep async transaction, cancellation, retry, idempotency, and ownership
  semantics explicit.
- Public items must satisfy workspace rustdoc lints. Comments explain reasons,
  constraints, compatibility, concurrency, or security rather than syntax.

## Persistence and testing

- PostgreSQL migrations are the persisted-model source of truth and become
  immutable after reaching `main`.
- Include `tenant_id` in tenant-owned key and reference paths. Enforce durable
  invariants in the database and reconstruct domain values through validating
  constructors.
- Avoid query-per-row loops. Use set-based operations when they preserve clear
  semantics and compile-time SQLx checking.
- Test observable behavior at the lowest convincing boundary. Defect fixes retain regression coverage.
- Tests must be deterministic, parallel-safe, tenant-aware where applicable,
  and independent of wall-clock sleeps and execution order.
- Use real PostgreSQL integration tests when constraints, transactions,
  locking, snapshots, or SQLx mapping are part of correctness.

## Safety and completion

Never commit secrets, customer data, production telemetry, machine-specific
paths, crash dumps, or unreviewed generated binaries. Do not present local
development defaults as production configuration.

The [coding harness guide](docs/development/harness.md) maps guidance to
executable feedback and explains which verification tier applies. Run the
complete applicable gate from
[`CONTRIBUTING.md`](CONTRIBUTING.md#verification).
Report passed, failed and not-run checks with their limitations.

## Agent skills

### Issue tracker

GitHub Issues in `stanimirivanov/fleetiq-platform`.
See [issue tracker](docs/agents/issue-tracker.md).

### Triage labels

Use the five configured roles. See [triage labels](docs/agents/triage-labels.md).

### Domain docs

Single-context glossary and repository-owned decisions.
See [domain docs](docs/agents/domain.md).
