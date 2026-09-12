# Contributing

## Change size

Each change should be reviewable as one pull request and deliver one coherent,
verified capability. Avoid creating unused abstractions for planned components.

An issue or pull request should state:

- the problem and resulting behavior;
- boundaries and deliberate exclusions;
- architecture decisions or contract changes;
- verification evidence and remaining limitations.

## Rust conventions

- Run formatting, Clippy with warnings denied, tests, and documentation before
  requesting review. The exact commands are in the root README.
- Prefer domain types over primitive strings at system boundaries.
- Make invalid states difficult to construct. Constructors validate invariants;
  persistence and transport adapters do not bypass them.
- Use ownership and borrowing directly. Introduce shared ownership, interior
  mutability, or asynchronous synchronization only for demonstrated needs.
- Prefer enums and exhaustive matching for closed alternatives. Return typed
  errors for expected invalid input.
- Keep traits at the boundary owned by their consumer. Add a trait when there are
  multiple implementations or a test seam with meaningful behavior.
- Keep modules cohesive and APIs small. Do not mirror class-oriented patterns
  mechanically when an enum, function, or newtype is clearer.
- Treat file length as a review signal rather than a limit. When production code
  approaches roughly 300 lines, check whether it contains concepts with separate
  reasons to change, or whether a function has become difficult to verify as one
  unit.
- Split a growing feature behind a small private facade. Keep supported imports
  stable with deliberate re-exports, and organize child modules around domain
  concepts or persistence responsibilities rather than placing each type in its
  own file.
- For persistence adapters, separate commands, queries, and row-to-domain
  reconstruction when those areas evolve independently. Keep transaction
  orchestration together and do not introduce a generic repository framework
  until repeated behavior has stable semantics.
- Move tests after establishing production boundaries. A separate test file
  improves navigation but is not by itself an architectural refactor.
- Document public APIs with purpose, invariants, errors, and examples where an
  example clarifies correct use. Explain reasons and constraints in comments;
  avoid narrating syntax.
- `unsafe` code is forbidden at workspace level. Any future exception requires a
  focused ADR, a narrowly scoped crate, documented invariants, and dedicated
  review.

## Architecture

Dependencies point inward: applications and adapters may depend on application
and domain crates; the domain crate remains free of infrastructure dependencies.
Protocol parsing identifies source observations. Semantic binding attributes
those observations to asset properties. Neither layer imports the other layer's
infrastructure adapters.

Use an ADR for choices that affect compatibility, durability, security, data
meaning, deployment topology, or more than one application.

## Protocol extension runtime

- Treat protocol-package code as untrusted input processing. Keep transport,
  credentials, admission identity, durable writes, and external side effects in
  the host.
- Install explicit fuel, wall-clock, and guest linear-memory limits for every
  WebAssembly invocation. Do not rely on sandboxing alone to bound resource use.
- Grant no host imports by default. Adding a filesystem, network, clock,
  environment, or other capability requires a focused contract, an ADR, and
  tests for its failure and resource behavior.
- Test packages with fragmented, coalesced, malformed, nonterminating, and
  allocation-heavy fixtures before activation.
- Describe each resource limit precisely. A WebAssembly linear-memory limit is
  not a bound on compiler memory, native stacks, host buffers, virtual address
  reservations, or total process memory.

## Database migrations

Follow the [SQL migration criteria](docs/development/sql-migrations.md). Treat
migrations as reviewed production code: document their domain intent, enforce
durable invariants in PostgreSQL, and verify the full migration chain against the
supported database version.

## PostgreSQL repository queries

- Treat database round trips as an explicit design cost. Batch homogeneous
  child rows with set-based PostgreSQL operations such as `UNNEST` instead of
  executing a query inside a loop.
- Prefer one atomic statement for an aggregate insert when a data-modifying
  common table expression can insert the parent and feed its key into a batched
  child insert. Use an explicit transaction when application-side validation or
  branching must occur between statements.
- Keep parent and child reads separate when that preserves clear absence
  semantics, ordinary `query_as!` compile-time field checking, and simple
  indexed access. Combine reads when measurements and expected cardinality
  justify the added aggregate-query and decoding complexity.
- Keep SQLx row types private to the adapter and reconstruct domain values
  through validated constructors.
- Document repository methods whose SQL shape is not obvious. Explain batching,
  transaction boundaries, locking, temporal selection, ordering, and deliberate
  multi-query decisions so reviewers do not need to infer them from SQL alone.

## Commits and generated files

Use clear, imperative commit subjects. Keep generated code reproducible and
commit it only when consumers need it without running the generator. Never commit
credentials, production telemetry captures, or customer-confidential equipment
definitions.
