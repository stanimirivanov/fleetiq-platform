# ADR 0100: Expose actor administration over HTTP

## Status

Proposed, 14 October 2026.

Implementation follows the approved M02 task; acceptance awaits human review.

Supersedes: none. Superseded by: none.

## Context

Actor provisioning, lifecycle administration, catalogues, and lifecycle audit
already exist as identity use cases and PostgreSQL adapters. Operators cannot
invoke them through the HTTP boundary. Browser writes need the same origin and
CSRF protection as logout. A lost HTTP response also exposes a retry gap: an
adapter cannot reproduce the original host receipt time, while existing immutable
application commands correctly include that time in exact equality.

## Decision

Expose four operations under the existing mutually exclusive session or workload
authentication modes:

- GET and POST /api/v1/admin/actors;
- POST /api/v1/admin/actors/{actor_id}/lifecycle; and
- GET /api/v1/admin/actor-lifecycle-changes.

Each operation obtains fresh authenticated actor evidence and consumes exact
platform identity.manage evidence in the existing identity service. Tenant hints,
tenant grants, and other platform actions grant no administration authority.
The server owns transport DTOs, body bounds, error redaction, and composition;
identity owns use cases and retry policy, and PostgreSQL owns atomic persistence.

Select the mutation guard from listener configuration, never from credential
headers. Browser commands and logout require a single exact configured Origin
and X-FleetIQ-CSRF: 1. Browser authentication rejects Authorization even with
valid cookies. Workload mode authenticates its explicit bearer and does not
require browser headers. No cross-origin permission is introduced.

Administrative JSON rejects unknown fields and is limited to 16 KiB. Clients
select opaque provisioning/change identities and target actor identities; these
are validated command data, never authentication evidence. The server derives
the administrator from authorization and the receipt time from its clock.
Clients supply the same command ID and business content after an uncertain result.

Add explicit for_delivery request constructors for these two identity commands.
Their first successful transaction selects the durable host receipt time.
Retries compare all command, administrator, and business identities but retain
the stored time. Existing new constructors retain strict timestamp equality.
No persisted columns, constraints, SQL statements, or immutable migrations change.
The existing concurrent claim/fresh-read and lifecycle serialization algorithms
resolve both delivery policies. A retry never reapplies a prior lifecycle state.

Catalogue pages use ascending actor IDs, default 50 and maximum 100.
Audit pages use descending (recorded_at_ms, change_id), with both cursor fields
required together. Pages have no cross-request snapshot guarantee. Provisioning
returns 201 initially and 200 on retry; lifecycle commands return the original
effect with a recorded/already_recorded disposition. Invalid requests return 400,
missing authentication 401, denied authority or browser mutation protection 403,
missing lifecycle targets 404, identity collisions or recovery lockout 409,
unsupported methods 405, and unexpected storage errors 500. Every outcome is
no-store; problems disclose no SQL, subjects, or credentials. Shared error-response
components now explicitly document the no-store header already returned by
protected operations; their problem bodies remain unchanged.

## Consequences

Existing atomic provisioning, no implicit grants, reversible activation,
immutable lifecycle audit, and final-administrator protection remain in force.
Deactivation takes effect on subsequent identity and policy checks; accepted
in-flight work is not cancelled. A deactivated caller cannot retry using its old
session, even when it originally performed the command.

HTTP delivery can recover from lost responses without trusting a client clock,
adding a receipt table, or relaxing exact upstream-command semantics. Application
repositories must honor the explicit recorded-time policy in addition to all
business fields. Tests exercise different proposed receipt times and retained
original effects, alongside the original strict-command suite.

The contract is additive, versioned as baseline.5. Rollback removes the routes
and leaves all durable identity and audit data intact. There are no new
dependencies, migrations, caches, invitations, identity-provider user creation,
grant administration, identity replacement, or administration UI.

## Alternatives considered

Accepting a client-provided audit timestamp confuses host facts with untrusted
input. Reading a timestamp before writing without a serialization/claim boundary
still races concurrent deliveries. Returning 409 for every later receipt time
does not support retries after a lost response. A separate HTTP receipt store
duplicates existing durable command identities. None is necessary here.
