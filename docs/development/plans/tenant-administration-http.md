# Tenant provisioning and lifecycle HTTP slice

Status: Implemented and verified.
Milestone: M02 - Product Foundation.

## TL;DR

Deliver tenant provisioning, activation/suspension, bounded platform catalogue
and immutable lifecycle audit after platform-access HTTP administration.
Keep tenant-access grant administration and UI in their later slices.

## Scope and boundaries

- Compose existing identity use cases and PostgreSQL repositories in a thin HTTP adapter.
- Preserve distinct exact ProvisionTenant and ManageTenantLifecycle evidence.
- Reuse listener-selected authentication and browser Origin/CSRF mutation protection.
- Add first-receipt delivery policy while retaining strict timestamp commands.
- Preserve atomic tenant, initial grants and provisioning evidence, and atomic
  lifecycle state/audit writes under existing transaction rules.
- Publish additive modular baseline.8, registered examples and executable contracts.
- Add no dependency, migration, role, frontend or ingestion runtime control.

## Acceptance and verification

Exercise concurrent deliveries with different host times, strict timestamp and
changed-administrator collisions, original effects, no-op audit, rollback and
missing initial actors. Prove provisioning retries do not recreate grants or
reactivate tenants, and old lifecycle commands cannot undo later transitions.

At HTTP/browser boundaries verify distinct current permissions, unauthorized
and malformed inputs, exact Origin/CSRF protection, mixed credentials,
suspension of discovery and fresh tenant reads, stable catalogue/audit cursors,
timestamp ties, no-store failures, registered response examples and absent
routes on an unconfigured listener.

Run focused identity, PostgreSQL, HTTP/browser and global contract suites, then
the full online Rust acceptance gate and authoritative OpenAPI group. Retain
explicit asynchronous race tests with bounded host test concurrency.
Record final verification evidence below before handover.

## Recovery and limits

See [ADR 0103](../../adr/0103-expose-tenant-administration-over-http.md) and
[the operating procedure](../../operations/tenant-administration.md). Suspension
controls fresh tenant authorization and discovery, not ingestion/worker execution
or already-issued evidence. Platform recovery remains available. Catalogue and
audit pages are traversals, not snapshots.

Rollback removes additive routes without deleting tenants, grants or audit.
Tenant-access administration APIs, UI and deployed provider/edge qualification
remain separate.

## Qualification results

Qualification on 2026-10-15 used Rust 1.98.1 and migrated PostgreSQL 18.6,
with SQLX_OFFLINE unset, RUST_TEST_THREADS=2 and CARGO_BUILD_JOBS=2. Explicit
asynchronous race tests remained enabled; no check, lint or policy was suppressed.

- Full authoritative Rust acceptance passed all twelve checks:
  `target/verification/acceptance-61256-1792072229837265100.json`.
- Authoritative OpenAPI verification passed all 64 sensor tests, lint and exact
  bundle freshness: `target/verification/openapi-59572-1792070769504723700.json`.
- All 121 server tests passed, including nine tenant HTTP cases and the browser
  mutation case. All eleven tenant provisioning/lifecycle repository tests, three
  new tenant contract tests, six global contract tests and 216 harness regressions
  passed within the complete workspace run.
- A structural comparison against the merged base proved all 23 prior paths and
  every prior component unchanged. Baseline.8 adds three paths and four operations.
- Migration bytes, SQL query text, offline metadata, dependencies and CI policy
  remain unchanged. No new security exception is needed.

Hosted CI, deployed OIDC/HTTPS qualification and ingestion suspension were not run.
