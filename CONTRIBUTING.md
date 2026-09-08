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

## Commits and generated files

Use clear, imperative commit subjects. Keep generated code reproducible and
commit it only when consumers need it without running the generator. Never commit
credentials, production telemetry captures, or customer-confidential equipment
definitions.
