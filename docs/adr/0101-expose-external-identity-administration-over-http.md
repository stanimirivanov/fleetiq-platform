# ADR 0101: Expose external-identity administration over HTTP

## Status

Proposed, 14 October 2026.

Implementation follows approved delivery-plan PR 5; acceptance awaits human review.

Supersedes: none. Superseded by: none.

## Context

Identity administration, current actor-scoped mapping pages, and immutable change
audit already exist behind exact ManageIdentity evidence. The HTTP boundary must
make them operable without implicit principal reassignment, final-identity loss,
subject disclosure in diagnostics, or a retry undoing later administration.
ADR 0100 established listener-selected mutation protection and explicit first-
receipt time policy for redelivered administrative commands.

## Decision

Expose three additive operations on the configured authenticated listener:

- GET /api/v1/admin/actors/{actor_id}/external-identities;
- POST /api/v1/admin/actors/{actor_id}/external-identity-changes; and
- GET /api/v1/admin/external-identity-changes.

Every request authenticates again and consumes exact current platform
identity.manage evidence. Tenant grants and other platform actions do not imply
this authority. Browser commands use the existing exact-Origin and CSRF guard;
workload mode remains a separately configured credential strategy.

The server validates bounded transport DTOs and derives administrator and time.
Identity owns command semantics and PostgreSQL retains its global serialization,
complete-content retry comparison, mapping mutation, and immutable audit transaction.
Add for_delivery to external-identity commands using the existing RecordedTime
policy. Exact retries retain the first durable host time and original effect;
existing new constructors still require timestamp equality. Resolve the prior
command before touching current mappings. No SQL statement or migration changes.

Commands carry a validated change ID, attach/detach kind, and exact authority and
opaque subject. They reject unknown fields and bodies above 16 KiB. Attach is
ensure-present and detach is ensure-absent for the exact target. Either operation
conflicts if the identity belongs to another actor; detach cannot remove a final
mapping. Authorized no-ops append no-change evidence. Missing mutation targets
return 404. Success returns recorded/already_recorded and the original state_changed;
collisions, implicit rebinding, and final-identity removal return 409.

Current pages use ascending (authority_id, subject) order, default 50 and maximum
100. Both after_authority_id and after_subject are required together. Missing
actors and actors without mappings intentionally share an empty terminal page,
matching the existing query contract. Audit pages use descending
(recorded_at_ms, change_id), with paired before cursor fields and the same bounds.
Neither query promises a cross-request snapshot or supplies authorization evidence.

Authorized responses contain exact subjects. Request DTOs have no Debug derive;
application command Debug implementations redact subjects like the existing
catalogue and audit types. All problems are generic and no-store, including parser
and storage failures. Raw query cursors also contain subject data: deployment
access logs must omit query strings, request bodies, and response bodies for these
routes. URL encoding provides syntax safety, not confidentiality. No credential
or subject logging is added by this adapter.

## Consequences

An administrator can inspect mappings, attach and verify a replacement login,
then explicitly detach a retired mapping without changing stable actor or grant
identity. The final-mapping check cannot prove a provider is reachable or that a
new mapping belongs to a usable account; the operator must verify replacement
access before retiring their own login.

A session retains its original exact mapping. Detaching it makes subsequent
authentication fail even while another mapping remains; accepted in-flight work
can finish. Retrying through a different identity is possible only when it
resolves to the same active administrator with current exact authority.

The baseline.6 HTTP contract is additive. No new dependency, migration, identity
provider operation, tenant/platform grant route, reverse-principal lookup, or UI
is introduced. Rollback removes the routes while retaining mappings and audit
facts. Strict upstream command callers retain their prior retry semantics.

## Alternatives considered

Using DELETE with a subject in the resource path would place principal data in
routine route logging and would not carry an explicit durable command identity.
A single audited command endpoint keeps attach/detach, retries, and effects explicit.

An opaque encoded catalogue cursor would still carry the subject and would not
protect it from query logging. Paired validated cursor fields reuse the exact
application tuple without a new codec, state store, or misleading secrecy claim.

Accepting a client receipt time, weakening all command-time equality, introducing
a second receipt table, and silently reapplying an old command were rejected.
The existing explicit receipt policy and serialized audit boundary suffice.
