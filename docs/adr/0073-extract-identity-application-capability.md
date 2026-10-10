# ADR 0073: Extract identity application capability

## Status

Accepted, 1 October 2026.

Acceptance reconciled from the approved architecture and harness sequence and
merged PR #201, commit `f9586e645489317438839f95522e3a7e49a85a4f`.
This records merge evidence; it does not claim independently enforced review
settings or infer acceptance from passing tests.

Supersedes: none. Superseded by: none.

## Context

The broad `fleetiq-application` crate owns actor, external-identity, tenant,
platform-grant, tenant-grant, audit, and bootstrap use cases beside unrelated
metadata and ingestion orchestration. Although access policy and evidence now
live in `fleetiq-access`, identity persistence adapters still import their
consumer-owned ports through the broad application facade. This obscures the
capability boundary and permits unrelated application modules to couple without
a crate-level dependency decision.

The existing transactional contracts, authorization evidence, public callers,
and one-process deployment must continue working during extraction.

## Decision

Create `fleetiq-identity` as an application capability depending only on
`fleetiq-access` and `fleetiq-domain`. Move the actor, external-identity,
platform-access, tenant-access, tenant-lifecycle, tenant-provisioning, and
platform-bootstrap use cases, their consumed repository ports, tests, and
private pagination mechanics into it. Keep the same public type names and
behavior. PostgreSQL production adapters implement the ports from the defining
crate directly, and the server composes bootstrap from that crate.

`fleetiq-application` depends on `fleetiq-identity` and temporarily re-exports
its public types for existing callers and integration tests. The exact workspace
dependency policy records every new or removed edge. No database migration,
runtime service, or action-vocabulary change accompanies this move.

## Consequences

Identity use cases can no longer import unrelated metadata or ingestion
application modules without a reviewed crate edge. Existing
`fleetiq_application` imports remain source-compatible while consumers migrate.
The new crate is a compile-time module boundary, not another deployed process.
Each crate keeps its fixed authentication test fixture private rather than
opening an evidence constructor for cross-crate tests.
Policy denial, typed authorization, atomic PostgreSQL transactions, audit
history, retry semantics, and tenant ownership remain unchanged.

PostgreSQL tests that still import the compatibility facade should move to
their defining capability as the facade is retired. The extraction adds a
workspace package and local dependency edges but no third-party dependency or
operational configuration. Rollback is source-level only.

## Alternatives considered

Keeping identity use cases as modules in the broad application crate would
leave the architectural boundary conventional rather than compiler-enforced.
Moving access policy into identity would reverse its current ownership and
force metadata consumers to depend on administration use cases. Splitting
identity into several crates now would create more edges before independent
deployability or variation justifies them.
