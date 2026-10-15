# Tenant-access HTTP administration

Status: Implemented and locally verified, 15 October 2026.

## TL;DR

Deliver tenant-scoped grant/revoke commands, one actor's current grants, and
immutable access-change history as one M02 slice. Reuse typed identity services,
current authorization, the selected browser/workload authentication boundary,
and atomic PostgreSQL persistence.

Milestone: M02 - Product Foundation.

## Scope and boundaries

- GET /api/v1/tenants/{tenant_id}/actors/{actor_id}/grants.
- POST /api/v1/tenants/{tenant_id}/actors/{actor_id}/access-changes.
- GET /api/v1/tenants/{tenant_id}/access-changes.
- Require exact current tenant access.manage for all three operations. No
  platform action, metadata permission or other tenant grant substitutes.
- Keep the server a transport adapter. Identity owns grant-query ports and retry
  policy; PostgreSQL owns checked SQL, tenant serialization and atomic audit.
- Preserve immutable migrations and existing strict command semantics. Add one
  checked catalogue query and its generated SQLx description; no dependency,
  schema, role vocabulary or deployment configuration change.
- Publish additive baseline.9 without changing existing operations or components.

## Behavior and failure model

Authenticate before request validation and authorize the routed tenant before
exposing actor, command or query errors. Browser commands require one exact
Origin and X-FleetIQ-CSRF: 1; bearer credentials cannot bypass browser mode.
Reject unknown fields, duplicate query parameters and bodies over 16 KiB.

Clients retain the same tenant-scoped change ID and business fields after an
uncertain result. The host derives tenant and administrator from consumed
authorization and supplies receipt time. Delivery retries return the original
effect and retain first committed time without restoring a subsequently revoked
grant. Strict commands still compare timestamps. Another administrator or changed
target, action or kind under the same tenant/change ID conflicts.

Catalogue reads are ascending by durable action name. Audit reads are descending
by recorded time and change ID, with both cursor fields required together.
Both are bounded to 1..100 entries, default 50; cursors are not authorization or
cross-request snapshots. Provisioning grants have separate audit evidence.

Preserve existing tenant policy: self-revocation, including the final tenant
administrator, is allowed. This slice adds no platform bypass or recovery API.
Operators must delegate and verify another active administrator before retiring
their own access. Self-revocation and suspension deny subsequent requests,
including retries; already issued evidence and in-flight work are not recalled.

## Verification plan

Use real PostgreSQL through composed HTTP routers to prove tenant isolation,
same change IDs in independent tenants, concurrent retries, no-op commands,
collision detection, missing targets, rollback on audit failure, current grants,
live authorization changes, deterministic pagination and browser mutation guards.
Explicitly test different receipt times and strict timestamp collision.

Validate all published examples, exact permission descriptions, closed actions,
safe error envelopes and no-store headers. Compare old paths and components
structurally with the preceding bundle. Run the complete thirteen-command
acceptance group and authoritative OpenAPI group; retain metadata-only evidence.

## Delivery and rollback

See [ADR 0105](../../adr/0105-expose-tenant-access-administration-over-http.md)
and the [operator procedure](../../operations/tenant-access-administration.md).
Rollback removes additive routes while retaining grants and immutable audit.
Administration UI, invitations, roles, batch changes, platform bypass, tenant
lockout recovery, ingestion, and provider qualification remain outside this slice.

## Qualification evidence

- Complete acceptance passed against trusted base d973482522d76df75779d7d2f9a825562486b452:
  all thirteen checks, 829 runtime tests across 60 binaries with zero skips,
  four compile-fail doctests, strict Clippy, online SQLx prepare check and rustdoc.
  Evidence: target/verification/acceptance-64340-1792086362748837900.json.
- Authoritative OpenAPI passed: 64 harness fixtures, lint and exact bundle
  freshness. Evidence: target/verification/openapi-39872-1792084582675898600.json.
- Structural comparison preserved all 26 preceding path definitions and all
  149 preceding components; the additive bundle has 29 paths.
- No new dependency or migration. All 28 published migrations remain unchanged.
- Initial builds failed during local disk exhaustion and cache recovery. After
  stopping the affected build tree and clearing inactive generated artifacts,
  the final complete run used local CARGO_INCREMENTAL=0 without changing any
  check, test coverage or Nextest concurrency. This does not change CI configuration.
- Hosted CI and deployment-provider qualification remain unexecuted locally.
