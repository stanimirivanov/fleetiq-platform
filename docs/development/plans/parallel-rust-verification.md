# Parallel Rust verification and dependency caching

Status: Implemented and locally qualified.

## TL;DR

Reduce Rust CI elapsed time through bounded cross-binary test execution and
dependency caching while retaining complete runtime, doctest and compiler gates.

Milestone: M01 - Decisions and Contracts.

## Scope and invariants

- Pin Nextest 0.9.145; run every workspace runtime test with all features and
  locked dependencies, followed by a separate required Cargo doctest check.
- Retain all other commands, warnings-as-errors, online SQLx, real PostgreSQL,
  explicit asynchronous races, and honest failure/not-run evidence.
- Use eight global slots, four PostgreSQL slots across postgres/server, two
  Wasmtime slots, zero retries and no fail-fast or selection filters.
- Validate the runtime configuration and strip ambient Nextest execution
  overrides. Preserve database and Cargo configuration.
- Restore dependency caches without caching workspace crates, installed binaries
  or verification reports. Save only on successful main pushes.
- Keep action major tags workflow-owned and setup mechanically verified.

## Qualification

Run positive and bypass fixtures for runtime configuration, ambient options,
exact tool versions, complete gate membership, cache ownership and installer
inputs. Check ordinary files and redirected configuration paths on Windows/Unix.

Reconcile the current Cargo and Nextest per-binary inventories, including
ignored status. Run the full Nextest suite and doctests against migrated
PostgreSQL. Measure build and execution separately, then run authoritative
acceptance. Compare hosted cold and warm CI after merge; local results do not
prove hosted performance.

## Recovery and limits

See [ADR 0104](../../adr/0104-bound-parallel-rust-tests-and-cache-dependencies.md)
and [the verification guide](../verification.md#parallel-test-execution-and-coverage).
Rollback restores the original Cargo runtime command and removes the separate
doctest command together. No product, schema or security exception changes.

No sharding, compiler-check parallelization, runner resizing or new performance
threshold is included. The resource ceilings are a conservative starting point;
new database-bearing packages must join the reviewed PostgreSQL group.

## Tool and action review

[Nextest 0.9.145](https://github.com/nextest-rs/nextest/releases/tag/cargo-nextest-0.9.145)
provides process-per-test scheduling; Cargo remains responsible for compilation
and doctests. The official Windows x64 archive was SHA-256 verified before local
installation. CI uses [install-action](https://github.com/taiki-e/install-action)
with checksums and no fallback. [Rust-cache](https://github.com/Swatinem/rust-cache)
provides dependency cache selection and Rust environment hashing. These are
development tools only; Cargo.lock, enabled crate features and runtime dependency
edges remain unchanged. Both action major tags remain workflow-owned.

The upstream cache implementation was inspected: shared-key overrides key,
so the workflow uses prefix-key and retains the explicit configuration hash.
Whole-target caching is disabled; explicit build, deps and fingerprint paths
exclude evidence, Nextest output and installed tool binaries independently of
upstream cleanup. Cold cache misses still execute every setup and gate step.

## Qualification results

Qualification on 2026-10-15 used Rust 1.98.1, Nextest 0.9.145 and migrated
PostgreSQL 18.6 on Windows. SQLX_OFFLINE was unset and builds used two jobs.

- Full authoritative acceptance passed all thirteen checks:
  `target/verification/acceptance-69936-1792082031259280700.json`.
- Authoritative OpenAPI passed 64 sensor tests, lint and exact bundle freshness:
  `target/verification/openapi-66836-1792081117227049100.json`.
- Independent discovery matched exactly: 816 tests across 59 Cargo/Nextest
  executables, identical test names and ignored status, zero ignored tests.
  Local metadata: `target/qualification/inventory-equivalence.json`.
- All 816 runtime tests passed with zero skipped, including 222 harness
  regressions and existing PostgreSQL concurrency/rollback tests. The resolved
  CI profile confirmed the shared resource-group assignment.
- All four compile-fail authorization doctests passed in their separate check.
- Nextest execution took 367.206 seconds, excluding its 6m05s build. These local
  measurements are qualification results, not a controlled before/after
  performance comparison or a hosted CI speedup claim.
- Generated CI consistency and source whitespace checks passed. No product
  code, existing product test, migration, query metadata or dependency lockfile
  changed.

Hosted Linux CI, action execution, and cold/warm cache effectiveness were not run
locally. Review the first cold run and a later main-seeded warm run before
claiming a CI time reduction. No retry, skip rule or security exception was added.
