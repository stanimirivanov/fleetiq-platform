# ADR 0105: Expose tenant-access administration over HTTP

## Status

Proposed, 15 October 2026.

Implementation follows the approved M02 delivery sequence; acceptance awaits human review.

Supersedes: none. Superseded by: none.

## Context

Tenant grant mutation and immutable audit already exist behind typed tenant
authorization. Product administration needs safe HTTP access plus a current-grants
query. Exact platform grants must remain distinct from tenant authority. Lost
responses must not make retries collide solely because host receipt time changes,
nor let a delayed grant retry restore subsequently revoked access.

## Decision

Expose GET /api/v1/tenants/{tenant_id}/actors/{actor_id}/grants, POST
/api/v1/tenants/{tenant_id}/actors/{actor_id}/access-changes and GET
/api/v1/tenants/{tenant_id}/access-changes. All consume exact current ManageAccess
tenant evidence. Derive tenant and administrator only from consumed evidence;
request bodies carry opaque tenant-scoped command IDs, grant/revoke and exact action.

The server composes identity services and PostgreSQL adapters. Identity owns a
narrow tenant-grant catalogue port; PostgreSQL reads it with one indexed bounded
keyset query. Keep current grants distinct from immutable change audit. Both
read surfaces use limits 1..100, default 50 and value cursors, not snapshots.
Missing target actors in catalogue reads and actors without grants are indistinguishable.
Mutation of a missing actor returns 404 and leaves no grant or audit.

Authenticate, enforce the listener-selected browser mutation guard, and authorize
the tenant before exposing actor, body or query validation. A tenant that cannot
be authorized returns 403. Reject unknown fields, bound JSON to 16 KiB, return
no-store responses and redact infrastructure details. Browser mode requires exact
Origin and X-FleetIQ-CSRF: 1 and never falls back to workload bearer.

Extend tenant commands with explicit first-receipt delivery policy. Existing exact
constructors retain timestamp equality. Under the existing per-tenant row lock,
compare every business and identity field, resolve stored results before mutation,
and retain original effect and receipt time on redelivery. Same IDs in different
tenants are independent. Different administrators or content in one tenant conflict.
No schema change or new dependency is required.

Preserve ADR 0044's current tenant-grant behavior, including self-revocation of the
final tenant administrator. Do not introduce a platform bypass, role hierarchy or
implicit grant. Document delegation and lockout explicitly. Self-revocation and
suspension deny fresh requests, including retries; issued evidence and in-flight
work are not recalled. Tenant lockout recovery remains separate product policy.

## Consequences

Tenant operators can administer exact access and inspect present grants and
attributed history through the selected authentication mode. Baseline.9 adds
three paths without changing existing payloads. Provisioning remains the source
of initial-grant evidence. PostgreSQL serializes low-volume tenant mutations;
independent tenants remain independent. Rollback removes routes and retains
durable data.

Administration UI, role/membership models, invitations, actor creation, batch
mutations, last-tenant-administrator protection and break-glass product recovery
are outside this decision.

## Alternatives considered

Reusing platform administration authorization would create an implicit tenant
bypass. Direct SQL in handlers would duplicate trusted attribution and atomicity.
Globally ignoring timestamps would change strict callers' command meaning.
Reapplying stored commands would undo later administrator decisions. Reconstructing
current grants solely from access-change audit would omit initial provisioning.
