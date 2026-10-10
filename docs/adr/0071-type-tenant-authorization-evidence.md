# ADR 0071: Type tenant authorization evidence by action

## Status

Accepted, 30 September 2026.

Acceptance reconciled from the approved architecture and harness sequence and
merged PR #195, commit `e6b0ce7d40758760a387afce7da2e6440b932a40`.
This records merge evidence; it does not claim independently enforced review
settings or infer acceptance from passing tests.

Supersedes: none. Superseded by: none.

## Context

ADR 0042 established exact tenant action grants and ADR 0047 required
authenticated actor evidence before authorization. The current
`AuthorizedTenantAction` is externally unforgeable but carries its action only
as a runtime enum. Every protected use case repeats an action comparison and
exposes a wrong-action error even though the operation's required action is
fixed. This leaves a routing mistake to fail at runtime after a caller has
obtained valid evidence for another action.

The persisted `metadata.read`, `metadata.manage`, and `access.manage` actions
remain the complete tenant policy vocabulary. PostgreSQL policy evaluation
already checks the exact actor, tenant, and action tuple on each request. No
schema, policy, or wire change is needed.

## Decision

`fleetiq-access` owns sealed zero-sized action markers `ReadMetadata`,
`ManageMetadata`, and `ManageAccess`. The markers map exhaustively to the
existing `TenantAction` enum. Authorization requests the marker as a type
parameter and returns `AuthorizedTenant<A>` only after the corresponding
current-policy decision allows the exact tuple. Its fields and constructor
remain private.

Protected tenant use cases accept only their required evidence type. Remove
their now-unreachable wrong-action errors and checks. Preserve tenant ownership
checks, policy denial and failure, single-use ownership, and repository errors.
Compile-fail documentation checks both incompatible action routing and
external evidence construction.

## Consequences

Passing a metadata-write grant to a metadata-read method, or a metadata grant
to tenant-access administration, is a Rust type error. The public Rust API
changes: callers must select a marker when authorizing and protected methods
now require `AuthorizedTenant<A>`; callers using the prior untyped API must
migrate. `fleetiq-application` re-exports the new access types for transitional
imports. Runtime access decisions and PostgreSQL grant encoding are unchanged.
No migration or deployment action is required; rollback is source-level only.

## Alternatives considered

Keeping the untyped evidence and repeated action checks preserves the routing
hazard. Runtime conversion from untyped to typed evidence would move, rather
than remove, the possibility of a wrong-action result and complicate the
trusted boundary. Public marker implementations would permit callers to define
actions outside the closed, audited policy vocabulary.
