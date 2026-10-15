# ADR 0104: Bound parallel Rust tests and cache reviewed build dependencies

## Status

Proposed, 15 October 2026.

Implementation follows the approved harness improvement; acceptance awaits human review.

Supersedes: none. Superseded by: none.

## Context

The Rust quality action takes approximately fourteen minutes in hosted CI.
Its complete test command runs separate integration executables sequentially.
Thirty-five PostgreSQL integration targets and server tests provide useful
cross-binary parallelism, while cold Wasmtime dependency builds add cost.
Increasing threads without resource limits can exhaust PostgreSQL connections.

## Decision

Replace the workspace runtime test check with pinned cargo-nextest 0.9.145.
Preserve the workspace, all-feature and locked dependency arguments. Add a
separate required Cargo doctest check, including compile-fail authorization
examples. All other checks retain their commands and order. Acceptance therefore
contains thirteen checks rather than twelve; the Rust group contains six.

Use eight global test slots, four shared PostgreSQL slots across the whole
postgres/server packages, and two Wasmtime slots. Package classification covers
future tests in those existing adapters. Local and CI profiles use the same
bounds. Disable retries and fail-fast; run the entire selected suite even when
a test fails. No default filter, skip rule or new ignored test is introduced.

Validate the parsed runtime configuration against reviewed policy before the
test command starts. Reject missing, redirected or changed configuration.
Remove ambient NEXTEST_* execution options from the inventoried test process;
fixed catalogue arguments and configuration own execution. Probe the exact
binary version and retain passed/failed/not-run evidence. Reconcile Cargo and
Nextest inventories during qualification; run existing explicit race tests.

CI uses taiki-e/install-action with an exact tool version, checksum verification
and no fallback. Use Swatinem/rust-cache for Cargo registries and dependency build
artifacts, excluding workspace crates and installed binaries. Cache identity
includes the Rust environment, manifests, lockfile, toolchain, OS, architecture,
flags, target and execution configuration. Only build, deps and fingerprint directories are included, excluding verification
reports, Nextest output and tool binaries. The prefix-key input preserves the
explicit cache key; shared-key would override that input. PRs restore; only successful main
pushes save. Cache hits never skip setup, migrations, verification or evidence.
Both actions use the existing workflow-owned major-tag policy.

## Consequences

Independent test binaries can overlap without multiplying database concurrency
without a bound. SQLx still creates isolated migrated databases. PostgreSQL and
Wasmtime ceilings are conservative starting points, not a throughput guarantee.
Keep compiler checks sequential to avoid competing builds on a single runner.

No database, product behavior, Rust dependency graph or security exception
changes. Hosted cold/warm cache benefits require actual CI measurement; local
timings cannot prove hosted speedup. Cache infrastructure is an optimization,
never acceptance evidence. Rollback restores the prior runtime test command and
removes its separate doctest check together, so coverage is retained.

## Alternatives considered

Unbounded threads can saturate database connections and CPU. Multiple CI shards
would duplicate cold builds and complicate complete-suite evidence. Merging
integration files solely to influence scheduling obscures capability boundaries.
Retaining Cargo for doctests preserves coverage unavailable in Nextest.
