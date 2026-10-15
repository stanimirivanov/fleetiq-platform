# Platform-access administration HTTP slice

Status: Implemented and verified.
Milestone: M02 - Product Foundation.

## TL;DR

Deliver the next administration slice after plan PR 5: explicit platform grant
and revoke commands, one actor's current grants, and bounded immutable audit.
Keep tenant-access administration and administration UI in their later slices.

## Scope and boundaries

- Compose existing identity services and PostgreSQL adapters under exact
  `ManagePlatformAccess` evidence; HTTP handlers own validation and DTOs only.
- Reuse listener-selected authentication and browser Origin/CSRF protection.
- Add explicit first-receipt delivery policy without weakening immutable commands.
- Retain serialized audit-before-mutation retry resolution and atomic rollback.
- Count only active actors when preserving the final platform-access administrator.
  Actor lifecycle and platform grant changes share the bootstrap singleton lock.
- Publish modular baseline.7 contracts, examples, compatibility tests and operations.
- No new dependency, migration, grant vocabulary, tenant route, or frontend change.

## Acceptance and verification

A PostgreSQL regression first reproduced revocation of the final active
administrator when an inactive actor retained a grant. It must now fail atomically.
Exercise concurrent redeliveries with distinct host times, strict timestamp
collisions, changed content/administrator collisions, original effect retention,
no-op audit, audit rollback, missing targets, action-name ordering, deleted
cursor continuation, timestamp ties, exact fresh authority and browser guard failures.

Run focused identity, PostgreSQL, HTTP, browser and contract tests, SQLx metadata
regeneration, the complete online Rust acceptance gate, and the OpenAPI group.
Use bounded host test concurrency without suppressing explicit asynchronous races.

## Recovery and limits

See [ADR 0102](../../adr/0102-expose-platform-access-administration-over-http.md)
and [the operating procedure](../../operations/platform-access-administration.md).
Self-revocation denies subsequent decisions, including retries; another current
administrator must inspect audit after an uncertain result. Previously issued
authorization evidence is not recalled. Pages are traversals, not snapshots.

Rollback removes the additive HTTP routes and retains grants and immutable audit.
Keep the corrected active-administrator guard. Production OIDC provider and HTTPS
edge qualification and tenant-access APIs remain outside this slice.

## Qualification results

Qualification on 2026-10-15 used migrated PostgreSQL 18.6, SQLX_OFFLINE unset,
RUST_TEST_THREADS=2 and CARGO_BUILD_JOBS=2. Explicit asynchronous races remained
enabled; no test, policy, advisory rule or threshold was suppressed.

- Full authoritative Rust acceptance passed all twelve checks:
  `target/verification/acceptance-59884-1792066176979622400.json`.
- Final authoritative OpenAPI verification passed all 64 sensor tests, lint and
  exact bundle freshness:
  `target/verification/openapi-60460-1792065436073350500.json`.
- All 111 server tests, eight platform-access repository tests, both new contract
  tests, six global contract tests and 216 harness regressions passed within the
  complete workspace run. Focused HTTP/browser, repository and contract suites
  also passed.
- The inactive-grant-holder regression failed before the guard correction and
  passed afterward. The shared lifecycle/revocation race preserves exactly one
  active recovery administrator and only the successful command's audit fact.
- All 20 previously published paths and every prior component retain their exact
  contracts. Baseline.7 adds three paths. The final bundle wording correction
  changes one new component description only; a structural comparison proved
  every payload definition unchanged.

The first focused build exhausted host storage while writing Windows debug
artifacts. Recovery removed only generated debug symbols and incremental compiler
cache in this repository, then reran verification. This required no source,
dependency, security exception or acceptance-policy workaround.

Hosted CI and deployed customer provider/HTTPS qualification were not run.
