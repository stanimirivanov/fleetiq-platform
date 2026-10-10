# ADR 0078: Retire application compatibility facade

## Status

Accepted, 1 October 2026.

Acceptance reconciled from the approved architecture and harness sequence and
merged PR #212, commit `43d009249db53df2f86bb4ab680bd7a04e32a890`.
This records merge evidence; it does not claim independently enforced review
settings or infer acceptance from passing tests.

Supersedes: none. Superseded by: none.

## Context

Access policy, identity administration, metadata use cases, ingestion
orchestration, and live projection contracts now live in their defining
capability crates. The remaining `fleetiq-application` package only re-exports
those types. PostgreSQL production adapters and the server already import
their defining crates directly. The facade's last consumers are PostgreSQL
integration tests.

Keeping a package whose only responsibility is compatibility obscures
ownership, adds a development dependency, and permits new callers to take a
broad edge instead of choosing the capability they use.

## Decision

Migrate every remaining workspace consumer to `fleetiq-access`,
`fleetiq-identity`, `fleetiq-metadata`, `fleetiq-ingestion`, or `fleetiq-live`
according to the type's defining crate. Remove the facade crate, its workspace
membership, the PostgreSQL test dependency, and its exact dependency-policy
entry. Keep the PostgreSQL adapter public API and all application and policy
contracts in their defining crates unchanged.

The architecture checker continues to reject policy-to-application
dependencies; its focused test now uses an existing application capability
rather than the removed facade. No database, SQL query, wire contract,
third-party dependency, or runtime behavior changes.

## Consequences

Workspace imports identify actual ownership and Cargo no longer builds a
transitive compatibility package. The former `fleetiq_application::*` path is
intentionally removed. The crate was unpublished and all in-repository callers
are migrated; out-of-repository source users would need to update imports to
the defining crates.

No data migration or rollout is required. Rollback would restore the facade
and test imports. The existing PostgreSQL integration suite remains the
authority for database behavior.

## Alternatives considered

Retaining the facade indefinitely would preserve a misleading broad entry
point and weaken capability review. Making it an empty deprecated package
would add a dependency with no behavior or useful migration signal. Combining
the capability crates would undo the ownership boundaries established by the
preceding extractions.
