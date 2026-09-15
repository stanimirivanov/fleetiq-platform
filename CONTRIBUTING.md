# Contributing

## Change size

Each change should be reviewable as one pull request and deliver one coherent,
verified capability. Avoid creating unused abstractions for planned components.

An issue or pull request should state:

- the problem and resulting behavior;
- boundaries and deliberate exclusions;
- architecture decisions or contract changes;
- the target GitHub milestone, named `MNN - Outcome` with numbering from `M01`;
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
- Install explicit fuel, wall-clock, guest stack, linear-memory, and
  table-element limits for WebAssembly execution. Do not rely on sandboxing
  alone to bound resource use.
- Reject a module that exceeds its configured raw-byte limit before passing it
  to a parser or compiler. Isolate compilation from telemetry-processing
  capacity and bound activation concurrency separately.
- Grant no host imports by default. Adding a filesystem, network, clock,
  environment, or other capability requires a focused contract, an ADR, and
  tests for its failure and resource behavior.
- Validate required export names, kinds, and signatures during module admission.
  Bind the admitted export to the compiled artifact so invocation cannot select
  a different entry point.
- Keep incremental framing side-effect-free. Return state changes, decoded
  outputs, and replies together only after processing one complete frame, then
  validate the whole proposed result before applying any part.
- Preserve package emission order for decoded outputs and replies. Output order
  participates in replay identity, and reply order determines transport writes
  after release conditions are satisfied.
- Bound both the number of package-proposed effects and their applicable byte
  totals. A per-item limit does not prevent excessive aggregate allocation.
- Stage fallible package effects in local host-owned values before changing
  session state. Do not expose partial results when a later validation can fail.
- Assign durable-message identities before append. Retry ambiguous failures with
  the exact same candidate, treat an identical existing value as success, and
  reject identity reuse when any immutable content differs.
- When persisting data derived from an accepted message, establish the parent
  fact with an exact-content `INSERT ... SELECT` or equivalent check. A matching
  foreign-key identity alone does not prove that caller-held device, receipt
  time, or payload evidence matches the durable message.
- Bind the exact processed frame, decoded outputs, and deferred replies into one
  pending acceptance value before asynchronous storage. Expose only replies
  already eligible after processing; assemble observations and release deferred
  replies only against matching durable acceptance evidence.
- Persist decoded output as one message-level result marker plus its complete
  ordered observation collection. Treat an empty collection as successful
  decoding, append the marker and children atomically, and resolve exact retries
  without creating duplicate observations or hiding identity collisions.
- Test packages with fragmented, coalesced, malformed, nonterminating, and
  allocation-heavy fixtures before activation.
- Describe each resource limit precisely. Linear-memory and table-element limits
  are not bounds on compiler memory, native stacks, host buffers, virtual address
  reservations, or total process memory.
- Validate a guest-provided pointer and length as one widened half-open range
  against current linear memory before every host read or write. Check reported
  output length before allocating its host buffer.
- Define how guest allocations are reclaimed on every success and failure path.
  A fresh per-invocation instance may use store destruction; reused instances
  require an explicit bounded cleanup and reset contract.
- Treat Wasmtime's maximum WebAssembly stack as an approximate engine-wide
  guard. It does not reserve native stack for the calling thread or bound stack
  consumed directly by host functions.
- Keep the checked-in WIT document authoritative for the protocol-package
  component boundary. Use explicit package versions, document every field's
  meaning and ownership, and generate bindings during normal compilation so
  incompatible interface edits fail the build.
- Treat values lifted through WIT as untrusted transport data. Convert them
  through semantic constructors, identify collection failures by stable
  zero-based position, and keep package-controlled values out of routine error
  and debug output.
- Admit components against generated bindings for the exact supported WIT world
  before activation. Apply artifact limits before parsing or compilation,
  reject undeclared host capabilities, and retain typed pre-instantiation
  bindings so message handling does not repeat compatibility discovery.
- Invoke each protocol component operation in a fresh bounded store and instance.
  Carry package continuity through explicit session-state bytes, distinguish typed
  package rejection from runtime failure, and validate lifted values before they
  reach host processing gates.
- Compose component processing with the trusted host gate using an extracted
  `WireFrame` and the same host-owned `SessionState` used for invocation. Do not
  expose the intermediate package proposal to downstream application code.
- Compose component framing with `SessionInputBuffer`. Invoke against its current
  unread bytes while holding exclusive buffer access, and let the host validate
  and remove at most one prefix after a successful package decision.
- Give each protocol session turn separate framing and processing budgets. Once
  framing extracts a prefix, return that exact `WireFrame` on processing failure
  instead of silently discarding or automatically reinserting it.

## Database migrations

Follow the [SQL migration criteria](docs/development/sql-migrations.md). Treat
migrations as reviewed production code: document their domain intent, enforce
durable invariants in PostgreSQL, and verify the full migration chain against the
supported database version.

For an idempotent append-only fact, use a stable tenant-scoped key, compare every
immutable field before treating a conflict as an exact retry, and reject row
mutation in storage. Do not infer idempotency from key equality alone.

When `INSERT ... ON CONFLICT DO NOTHING` waits for a concurrent writer, do the
fallback read in a new `READ COMMITTED` statement. A fallback `SELECT` inside the
same data-modifying common table expression can retain an older snapshot and
miss the row that caused the conflict. Do not force visibility through a no-op
update on an immutable table.

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
- At PostgreSQL `READ COMMITTED`, do not join dependent rows into a locking query
  when the locked row may change which dependent row is current. Lock the stable
  identity first, then read its dependent row in a new statement with a fresh
  snapshot.

## Commits and generated files

Use clear, imperative commit subjects. Keep generated code reproducible and
commit it only when consumers need it without running the generator. Never commit
credentials, production telemetry captures, or customer-confidential equipment
definitions.
