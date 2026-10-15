# ADR 0103: Expose tenant provisioning and lifecycle administration over HTTP

## Status

Proposed, 15 October 2026.

Implementation follows the approved M02 delivery sequence; acceptance awaits human review.

Supersedes: none. Superseded by: none.

## Context

Tenant provisioning, lifecycle administration, catalogue and lifecycle audit
already exist behind typed platform authorization. HTTP access must preserve
atomic provisioning, separate permissions and safe retries. Suspension must
prevent fresh tenant access while leaving platform recovery available.

## Decision

Expose GET and POST /api/v1/admin/tenants, POST
/api/v1/admin/tenants/{tenant_id}/lifecycle, and GET
/api/v1/admin/tenant-lifecycle-changes. Provisioning consumes exact ProvisionTenant
evidence; catalogue, lifecycle and audit consume ManageTenantLifecycle evidence.
Other platform actions and tenant grants never substitute.

The server is a thin adapter composing existing identity services and PostgreSQL
repositories. Authenticate, enforce listener-selected browser mutation protection,
and authorize before reporting malformed input. Reject unknown JSON/query fields,
cap bodies at 16 KiB, and return no-store results with safe problem details.
Derive the administrator and receipt time on the server.

Provisioning atomically creates an active tenant, its immutable command fact and
exact metadata.read, metadata.manage and access.manage grants for an existing
internal actor. The provisioner receives no implicit tenant grant. Existing
inactive actors may retain grants but cannot exercise them. This preserves the
application contract; operators must choose and verify an active administrator.

Use explicit for_delivery requests with first-durable-receipt time. Strict new
commands still compare exact timestamps. PostgreSQL must compare command identity,
authenticated administrator, tenant and all business fields before returning a
retry. It must resolve the original durable result before mutating current state.
Provisioning retries do not recreate revoked grants or reactivate suspended
tenants; lifecycle retries do not undo a later transition. No-op lifecycle commands
record their own immutable effect. Existing transaction locks and unique arbiters
remain authoritative; no migration or new lock is required.

Catalogue pages use ascending tenant ID and include suspended tenants; lifecycle
audit pages use descending (recorded_at_ms, change_id). Limits are 1..100, default
50. Continuations are value cursors, not authorization or traversal snapshots.

## Consequences

Platform operators can establish and suspend tenant boundaries without direct
database edits. Suspension removes tenants from fresh operator discovery and
denies fresh tenant authorization while retaining data, grants and audit. It
does not recall issued authorization, cancel in-flight work or suspend ingestion
and workers. Platform lifecycle administration remains available for activation.

Baseline.8 adds three path items and four operations without changing previous
payloads. No tenant-grant API, UI, role model, dependency or schema change is
included. Rollback removes these routes while retaining durable tenant state
and immutable audit.

## Alternatives considered

Sharing one platform permission would erase an existing authorization boundary.
Accepting caller timestamps would weaken trusted audit context. Relaxing strict
commands globally would change non-HTTP callers; the explicit delivery policy
retains both contracts. Reapplying retries would undo subsequent administration.
