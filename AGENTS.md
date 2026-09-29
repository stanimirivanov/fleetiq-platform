# Repository working agreement

## TL;DR

Deliver one verified pull-request-sized capability at a time. Preserve inward
dependencies, trusted-host boundaries, tenant ownership, deterministic replay,
and immutable history. Follow the Rust and SQL standards and report every check
exactly.

This is the concise entry point for coding tools and contributors.
[`CONTRIBUTING.md`](CONTRIBUTING.md) is the canonical workflow policy. The
[documentation map](docs/README.md) routes each task to the additional sources
that apply.

## Before changing anything

1. Read [`CONTRIBUTING.md`](CONTRIBUTING.md), then use the
   [documentation map](docs/README.md) to load the product, architecture,
   development, operations, and accepted-decision sources relevant to the
   task. Do not bulk-read unrelated guides or ADRs.
2. Inspect the branch and working tree. Preserve pre-existing changes and keep
   unrelated work out of the task.
3. Check [current state](docs/development/current-state.md) before assuming a
   planned capability already exists.
4. Read [SQL migration criteria](docs/development/sql-migrations.md) before
   changing persistence.
5. Use repository-pinned tools. A skipped or unavailable check is not a pass.

If code, documentation, and an accepted ADR disagree, correct an obvious local
error or propose a superseding ADR. Do not silently choose one source.

## Shape of work

- One task and pull request delivers one coherent, independently reviewable
  capability.
- Prefer a thin end-to-end slice over an unused layer or speculative framework.
- Keep refactoring, dependency updates, generated churn, and formatting out of
  behavioral changes unless the change requires them.
- Add no crate, service, queue, cache, datastore, abstraction, or dependency
  without behavior in the current change that requires it.
- State scope, exclusions, compatibility effects, assumptions, risks, and
  verification evidence.

## Architecture and Rust

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
- Test observable behavior at the lowest convincing boundary. Defect fixes
  begin with a regression test.
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
Finish with its prescribed completion report, including the repository,
milestone, copy/paste-ready issue text, limitations, and passed/failed/not-run
checks.
