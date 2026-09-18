# SQL migration criteria

## TL;DR

- Migrations are the source of truth for persisted FleetIQ models and become
  immutable after reaching `main`.
- Keep each migration focused, tenant-safe, forward-compatible, and explicit
  about domain invariants, ownership, mutability, and operational risk.
- Document schemas for reviewers and database operators, not only for SQL
  experts.
- Verify the full chain on PostgreSQL 18.6 and compile SQLx checked queries
  against the resulting schema.
- Regenerate and commit `.sqlx` offline query descriptions whenever a migration
  or checked query changes.

PostgreSQL migrations are the source of truth for FleetIQ's persisted metadata
model. Every migration must be understandable and reviewable without requiring
the reader to infer business rules from application code.

## Scope and evolution

- Keep each migration focused on one coherent capability or one necessary data
  transition. Split unrelated schema changes into separate migrations.
- Treat a migration as immutable after it reaches `main`. Correct a deployed
  schema through a new forward migration rather than rewriting history.
- Prefer additive, backward-compatible changes. Use an expand, migrate, contract
  sequence when a rolling deployment could run old and new application versions
  at the same time.
- Document lock duration, table rewrites, backfill strategy, and operational
  recovery for changes that can affect an existing production data set.
- Keep migrations transactional. Isolate and explain any PostgreSQL operation
  that cannot run in the migration transaction.

## Schema design

- Qualify product objects with their schema name. Use `snake_case` for schemas,
  tables, columns, functions, constraints, and indexes.
- Give constraints and indexes stable descriptive names, using the prefixes
  `pk_`, `fk_`, `uq_`, `ck_`, and `ix_`. Error mapping may depend on these names.
- Declare nullability deliberately. Add defaults only when the database can
  assign a value with the correct domain meaning.
- Enforce durable invariants with primary keys, foreign keys, unique constraints,
  and check constraints. Application validation improves errors but does not
  replace database enforcement under concurrency.
- Include `tenant_id` in tenant-owned primary, unique, and foreign-key paths.
  Composite foreign keys must prevent a row from referencing another tenant's
  data.
- State deletion behavior explicitly when child rows are owned by an aggregate.
  Use cascading deletion only when the child has no independent lifecycle.
- Store timestamps that represent instants as `TIMESTAMPTZ` by default. An
  explicit domain or wire contract may use an integer epoch representation when
  the migration documents its unit, range, and conversion boundary. Choose exact
  numeric types where floating-point rounding would change business meaning.
- Add indexes for demonstrated query, join, ordering, or foreign-key access
  patterns. Explain indexes whose purpose is not evident from their columns.

## Documentation

- Begin each migration with a summary of the capability, the storage concepts it
  introduces, and the important ownership, mutability, and tenancy rules.
- Divide substantial migrations into named sections so reviewers can follow
  dependency order.
- Introduce every table, function, trigger, and non-obvious index with comments
  that explain its domain purpose and design consequences.
- Explain non-obvious columns and constraints next to their definitions. Focus on
  why a rule exists, including `NULL` semantics, version pinning, and delete
  behavior, instead of restating SQL syntax.
- Add `COMMENT ON` statements for schemas, tables, functions, and columns whose
  meaning operators and analysts need when inspecting the database catalog.
- Keep comments accurate when DDL changes. Misleading documentation is a schema
  defect.

## Safety and verification

- Never include credentials, customer data, or production telemetry in a
  migration or fixture.
- Review PostgreSQL privileges and row-level security when a migration introduces
  a new access boundary. Application tenant filters do not replace database
  authorization where direct access is possible.
- Apply the complete migration chain to an empty database running the supported
  PostgreSQL version.
- Compile all SQLx checked queries against the migrated schema.
- Run `cargo sqlx prepare --check --workspace -- --all-targets --all-features`
  against that schema. The committed metadata must describe the same queries
  used by database-independent container builds.
- Add integration tests for constraints, transactions, tenant isolation, and
  row-to-domain mapping introduced or changed by the migration.
- Run formatting, strict Clippy, the full test suite, and documentation checks
  before review. Record any production rollout or recovery step that cannot be
  verified automatically.
