# Engineering standards

## TL;DR

- Preserve capability boundaries and point dependencies toward domain meaning.
- Use idiomatic Rust types and ownership to make invalid states difficult to
  represent.
- Keep untrusted input, SQL rows, WIT values, and transport types at adapters.
- Test observable behavior and relevant failure paths deterministically at the
  lowest convincing boundary.
- Document purpose, invariants, errors, concurrency, and security constraints;
  comments should explain why rather than narrate syntax.

## Applicability

Normative terms use the meanings in
[`CONTRIBUTING.md`](../../CONTRIBUTING.md#policy-language-and-sources-of-truth).
These standards apply to production code, tests, build scripts, generated
bindings, migrations, and operational tooling. Workspace lint configuration and
`rustfmt` own mechanical Rust style.

The goal is maintainable correctness, not pattern quotas. Record an exception
to a strong default when its trade-off matters to review. Use an ADR when it
establishes a durable or cross-component choice.

## Architecture

Organize code around ownership and capabilities:

- domain crates own vocabulary, invariants, values, and state transitions;
- application code coordinates use cases and owns the ports it consumes;
- policy crates implement protocol-host, semantic, and analytics decisions;
- inbound adapters validate transport shape and create application commands;
- outbound adapters implement ports and translate domain values to storage or
  provider representations; and
- composition roots select concrete implementations and configuration.

Dependencies point inward. Domain code does not import SQLx, Wasmtime, HTTP,
brokers, object storage, identity providers, or vendors. A crate owns its
writes; cross-capability behavior uses an application port or versioned event,
not another crate's private table.

The [workspace dependency rules](../architecture/dependency-rules.md) make
direct crate edges executable policy. Add or change an internal dependency only
with its role, direction, and exact policy entry in the same pull request.

Protocol decoding identifies source observations. Semantic binding attributes
them to asset properties. Keep wire DTOs, WIT-generated values, database rows,
events, and domain values distinct when their invariants or evolution differ.

### SOLID in Rust

- Give a type or module one cohesive reason to change at its scale; do not
  force one type per file.
- Introduce an extension point only when a real variation or consumer-owned
  test seam requires it.
- Trait implementations preserve success, absence, failure, ordering,
  concurrency, and ownership semantics.
- Keep traits narrow and place them with the consumer. Prefer static dispatch
  until runtime polymorphism is needed.
- Prefer composition, enums, newtypes, and exhaustive matching. Avoid generic
  repository frameworks and vague `Manager`, `Helper`, `Util`, or `Processor`
  names.

Treat file length as a review signal. Around 300 production lines, check for
separate concepts or responsibilities. Split around stable domain or adapter
boundaries behind a small facade and deliberate re-exports. Moving tests alone
can improve navigation but is not an architectural refactor.

## Rust

### Types, ownership, and APIs

- Use validated newtypes for identifiers, versions, units, time, sizes, and
  other primitives with domain meaning.
- Use enums for closed alternatives and state machines. Avoid boolean groups
  that permit impossible combinations.
- Accept borrowed values where ownership is unnecessary. Transfer ownership
  when a caller must retain the exact value for asynchronous retry.
- Add `Arc`, interior mutability, locks, and channels only with demonstrated
  shared ownership or concurrency requirements.
- Keep public APIs small. Make fields private when construction must preserve
  invariants, and route reconstruction through validating constructors.
- Mark meaningful results `#[must_use]` where ignoring them could lose state,
  work, or a required decision.
- `unsafe` is denied at workspace level. The sole owner-approved exception is
  [generated guest ABI glue](../architecture/dependency-rules.md#generated-guest-abi-boundary),
  guarded by the architecture sensor. Any further exception needs a focused
  ADR, a narrowly scoped crate, documented invariants, and dedicated review.

### Errors and boundaries

- Use typed errors for expected invalid input and operational failures.
- Distinguish absence, conflict, identity collision, package rejection,
  resource exhaustion, cancellation, timeout, and infrastructure failure when
  callers act differently.
- Preserve causes when wrapping unexpected failures. Do not leak SQL, runtime,
  payload, or credential details across public boundaries.
- Avoid panics in library behavior reachable from untrusted data. Assertions
  are appropriate for proven internal invariants and tests.
- Keep package-controlled values out of routine `Debug`, errors, logs, and
  metrics.

### Async, time, and concurrency

- Do not hold synchronous locks or borrowed database transactions across
  unrelated awaits.
- Document transaction isolation, lock order, cancellation priority, delivery
  guarantees, ownership transfer, and retry safety where they affect behavior.
- Store instants in UTC and distinguish device event time, host receipt time,
  recorded time, and evaluation time.
- Bound payloads, collections, retries, concurrency, execution time, and
  memory-heavy work before allocation when possible.
- Do not coordinate tests with sleeps. Use barriers, observable state,
  controllable clocks, or protocol acknowledgements.

### Documentation and modules

Public items document purpose, invariants, parameter and return constraints,
meaningful errors, and safety or concurrency behavior. Examples are compiled
when practical and included only when they clarify correct use. Comments explain
reasons, compatibility, ownership, query shape, or non-obvious limits.

Crate roots use `//!` documentation for the crate contract. A crate README is
added only for independent setup, configuration, deployment, publication, or
external contribution. Do not duplicate rustdoc in a README.

## Persistence

Follow [SQL migration criteria](sql-migrations.md). Keep SQLx row types private
to the adapter and reconstruct domain values through validated constructors.

Treat round trips as a design cost. Batch homogeneous rows with set-based SQL,
such as `UNNEST`, instead of a query loop. Prefer one atomic statement when it
keeps semantics and compile-time checking clear. Separate reads are acceptable
when they preserve simple absence semantics, ordinary `query_as!` column
checking, and indexed access.

At PostgreSQL `READ COMMITTED`, a data-modifying common table expression can
retain a snapshot that does not see the row causing `ON CONFLICT` to wait. Use a
new statement for conflict resolution when fresh visibility is required. Lock a
stable revision identity before reading dependent current state when the lock
can change which dependent row is current.

When several unique constraints cover the same immutable identity, a targeted
`ON CONFLICT` clause handles only its selected arbiter and can expose another
constraint during concurrent identical inserts. Use unqualified `ON CONFLICT DO
NOTHING` only when every suppressible uniqueness conflict can be resolved by
the same fresh-snapshot identity read and complete-content comparison. Keep a
targeted arbiter when another unique constraint represents a distinct conflict
that callers must observe.

For closed action vocabularies persisted as text, keep encoding and decoding
in one adapter-owned codec. Make encoders exhaustive, test every exact stored
name and round trip, and preserve table-column context when persisted values
cannot be reconstructed.

Document repository methods whose batching, transaction, locking, temporal
selection, ordering, or deliberate multi-query shape is not obvious.

## Contracts and compatibility

Rust public APIs, WIT worlds, events, configuration, database migrations,
persisted payloads, protocol packages, and HTTP schemas are compatibility
boundaries.

- Version semantics, not filenames alone.
- Additive changes define defaults and old-reader behavior.
- Breaking changes need a new version or expand/migrate/contract plan, tests,
  rollout and rollback notes, and usually an ADR.
- Pin every package, binding, definition, calculator, or policy revision needed
  to explain and replay a result.
- Commit generated code only when consumers cannot reasonably generate it and
  regeneration is deterministic.

## Testing

- Test behavior at the lowest boundary that proves it: domain unit tests,
  application tests with fakes, adapter integration tests against real
  protocols, contract tests, and a small number of end-to-end scenarios.
- Name tests for the condition and observable result.
- Begin defect fixes with a test that fails before the fix.
- Cover relevant success, invalid input, absence, duplicate delivery, stale
  revision, tenant isolation, timeout, resource limit, retry, partial failure,
  concurrency, and recovery paths.
- Use PostgreSQL integration tests when constraints, transactions, snapshots,
  locking, encodings, or SQLx mappings are part of correctness.
- Keep default CI independent of physical devices, paid services, and mutable
  external systems. Hardware qualification uses an explicit later-stage suite.
- Never add blind retries or increase timeouts without diagnosing the source of
  nondeterminism.

## Security and operations

- Deny capabilities by default and apply least privilege.
- Treat device bytes, package artifacts, lifted component values, API input,
  configuration, and repository content as untrusted.
- Keep credentials in approved secret injection. Never log credentials, raw
  payloads, or customer-confidential asset data.
- Authenticate identity and authorize tenant and asset access at application
  and protected-data boundaries.
- Use bounded-cardinality structured logs, traces, and metrics. Domain IDs may
  be log fields when policy permits; do not use them as unbounded metric labels.
- Dependencies need a current capability, compatible license, maintained
  release, proportionate security review, pinned resolution, and exit path.
- New runtime behavior documents healthy signals, failure classes, recovery,
  rollout, rollback, and ownership.

## Primary references

- [The Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [The Rust Reference](https://doc.rust-lang.org/reference/)
- [Rust Clippy lints](https://rust-lang.github.io/rust-clippy/)
- [SQLx documentation](https://docs.rs/sqlx/)
- [PostgreSQL documentation](https://www.postgresql.org/docs/)
- [Wasmtime security](https://docs.wasmtime.dev/security.html)
