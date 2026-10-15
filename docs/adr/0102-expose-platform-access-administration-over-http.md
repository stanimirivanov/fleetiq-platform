# ADR 0102: Expose platform-access administration over HTTP

## Status

Proposed, 15 October 2026.

Implementation follows the approved M02 delivery sequence; acceptance awaits human review.

Supersedes: none. Superseded by: none.

## Context

Platform grant administration, actor-scoped current grants and immutable audit
already exist behind exact `ManagePlatformAccess` evidence. They need an HTTP
adapter that preserves lockout protection, fresh authorization and safe delivery
retries. ADRs 0100 and 0101 established listener-selected mutation protection
and an explicit first-receipt policy alongside strict immutable commands.

Review exposed a gap: the grant-revocation guard counted inactive actors even
though current policy cannot authorize them. Delegating to an inactive actor
then revoking the last active administrator could eliminate recovery authority.

## Decision

Expose three additive authenticated operations:

- GET /api/v1/admin/actors/{actor_id}/platform-grants;
- POST /api/v1/admin/actors/{actor_id}/platform-access-changes;
- GET /api/v1/admin/platform-access-changes.

Every operation consumes fresh exact platform.access.manage evidence. Other
platform actions and tenant grants do not substitute. Browser commands reuse
the configured exact-Origin and CSRF-marker guard; workload authentication is
a separate listener mode. DTOs reject unknown fields, enforce closed action and
grant/revoke vocabularies, and cap JSON at 16 KiB. Server-derived administrator
and receipt time cannot be supplied in the body.

Identity owns explicit for_delivery requests through RecordedTime. PostgreSQL
compares administrator, target, action, kind and command identity before current
state mutation. A retry preserves the first committed time and original effect;
strict new requests still compare timestamps. Never reapply an earlier grant
after a later revoke. No-op commands append their own immutable audit fact.

Revocation must leave an active platform-access administrator. Count grants
joined to active actors under the existing bootstrap singleton transaction lock,
which actor lifecycle changes also acquire. No migration or second lock is needed.
Inactive actors retain grants but provide no effective recovery authority.

Current grants use ascending durable action names and optional after_action;
audit uses descending (recorded_at_ms, change_id) with paired before cursor fields.
Both default to 50 and cap at 100. Missing actors and actors with no grants return
an empty terminal catalogue page, matching the existing query port. Missing
mutation targets return 404. Collision or final-administrator revocation returns
generic 409. Every result, including parser/storage problems, is no-store and
does not expose credentials, SQL or constraint details.

## Consequences

Authorized operators can delegate and retire exact platform privileges without
direct database editing. Self-revocation makes later decisions fail, including
an exact retry. Recovery after an uncertain result requires another authorized
administrator to inspect audit and current grants. Previously issued authorization
evidence can finish; there is no transactional recall of in-flight work.

Current grants and change audit remain distinct; initial bootstrap grants have
separate evidence. Cursors do not promise a snapshot or provide authorization.
The exhaustive HTTP action conversion and contract tests force explicit review
when the application vocabulary changes.

The baseline.7 contract is additive. No tenant-access administration, tenant
lifecycle/provisioning route, role system, provider management or UI is included.
Rollback removes routes while retaining durable grant and audit data and the
active-administrator guard correction.

## Alternatives considered

Accepting client audit times or relaxing all immutable command timestamps would
weaken existing contracts. Reusing first-receipt policy avoids both. Replaying
old mutations would undo later administration and is incompatible with durable
idempotency. Counting every retained grant would preserve the reproduced lockout.
