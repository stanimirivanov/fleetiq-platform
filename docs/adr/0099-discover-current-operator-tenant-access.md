# ADR 0099: Discover current operator tenant access

## Status

Proposed, 14 October 2026.

Implementation follows the approved M02 task;
acceptance of this durable decision awaits human review of the pull request.

Supersedes: none. Superseded by: none.

## Context

Connected operators currently need a tenant ID before attempting protected
metadata reads. The platform tenant catalogue requires administration authority
and includes suspended tenants; it is unsuitable as a self-service selector.
Sessions intentionally expose only actor identity and expiry.

## Decision

Add GET /api/v1/me/tenants under the listener's existing mutually exclusive
browser-session or workload authentication. The access capability owns a narrow
discovery read port and service; PostgreSQL supplies one checked, set-based query.
The service derives the subject from consumed authentication evidence. HTTP
accepts no actor, role, tenant impersonation, or platform-authority parameter.

Return only active tenants with explicit grants for the current active actor.
Return exact tenant actions without implication, hierarchy, or platform grants.
Order tenants by stable ID and actions by persisted action name. Limit pages to
1..=100 tenants (default 50), using an exclusive tenant-ID after cursor and one
lookahead tenant. Group before limiting so action sets cannot be split.
A cursor is a position, including after revocation; it is not access evidence.

Every page reads current committed policy in a single statement snapshot.
Traversal has no cross-request snapshot guarantee. Entries confer no authority:
protected operations independently authenticate and authorize. An empty set
returns 200 with an empty tenants array and null next_after. Invalid credentials
return 401, invalid queries 400, and unavailable policy 500. All responses are
no-store; diagnostics disclose no storage details.

Publish additive modular OpenAPI schemas, parameters, examples, and operation
listOperatorTenants. Preserve session and existing metadata payloads. Reuse
current tables and indexes, without migrations, dependencies, or grant caching.

## Consequences

Clients can offer bounded tenant selection and capability-aware controls.
Revocation, actor deactivation, and tenant suspension affect subsequent requests;
already accepted in-flight operations retain existing semantics.
Discovery may include manage-only tenants even without metadata.read.
Platform administrators do not discover tenants unless explicitly granted access.
A retained UI hint can become stale immediately and must never authorize work.

The endpoint is absent on unconfigured listeners. Rollback removes this additive
route; clients must handle older deployments without inventing permissions.
Frontend adoption is a separate PR pinned to the merged contract revision.

## Alternatives considered

Embedding grants into sessions makes hints stale and changes the established
session payload. Using the administrative catalogue exposes unrelated tenants.
Querying each tenant's policy separately adds N+1 queries and inconsistent
per-page snapshots. All are unnecessary for this actor-scoped read.
