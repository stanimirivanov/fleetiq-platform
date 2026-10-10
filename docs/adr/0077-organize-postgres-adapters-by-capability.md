# ADR 0077: Organize PostgreSQL adapters by capability

## Status

Accepted, 1 October 2026.

Acceptance reconciled from the approved architecture and harness sequence and
merged PR #210, commit `c9e3b18e7d8c75e046bb16a75943a3584d520004`.
This records merge evidence; it does not claim independently enforced review
settings or infer acceptance from passing tests.

Supersedes: none. Superseded by: none.

## Context

The application ports now belong to access, identity, metadata, and ingestion
capability crates, but `fleetiq-postgres` still declares every adapter in one
flat crate root. That layout makes ownership and review scope harder to see.
Platform and tenant action names are also repeated across exact policy reads,
grant administration, audit reconstruction, and grant catalogues. A new action
could be encoded or decoded inconsistently in those paths.

The database remains one transactional boundary. Public adapter names,
checked SQL, persisted action strings, column-specific corruption errors, and
existing behavior must remain stable.

## Decision

Organize the existing PostgreSQL adapters into private `access`, `identity`,
`metadata`, and `ingestion` modules that mirror their inward capability
boundaries. Keep the same public adapter types and aliases at the crate root.
Retain the shared database error type and embedded migrator at that root.

Add one adapter-owned action codec for exact platform and tenant action names.
Policy reads and grant writes use its exhaustive encoders. Audit and catalogue
reconstruction use its decoders and supply their original column paths for
corruption diagnostics. Test every current action against its exact persisted
name in both directions and reject unknown names with the caller's column
context.

The few action-name literals inside checked SQL express operation-specific
bootstrap grant sets and last-administrator safeguards, rather than general
Rust-to-storage conversion. They remain explicit in this behavior-preserving
change; altering those queries requires a separate SQLx metadata update.

No migration, SQL query, external dependency, new crate, or deployment change
is required.

## Consequences

Reviewers can locate an adapter beside related adapters without confusing the
PostgreSQL crate's internal organization with separate databases or services.
One exhaustive match per action vocabulary is the review point when an action
is added. Existing public imports and database behavior remain unchanged.

The codec remains private to PostgreSQL; domain and access policy do not learn
storage spellings. The existing integration suite remains the authority for
transactional and SQLx behavior. Rollback is source-only.

## Alternatives considered

Splitting PostgreSQL into capability crates would complicate shared
transactions and migrations before independent storage lifecycles exist.
Keeping flat modules retains navigational coupling. A generic string-enum
framework would obscure the small closed vocabularies and their field-specific
corruption diagnostics.
