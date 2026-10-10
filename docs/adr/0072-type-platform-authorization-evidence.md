# ADR 0072: Type platform authorization evidence by action

## Status

Accepted, 1 October 2026.

Acceptance reconciled from the approved architecture and harness sequence and
merged PR #197, commit `757909742f0e1eaa7849e7fcc5329589ca6bda2c`.
This records merge evidence; it does not claim independently enforced review
settings or infer acceptance from passing tests.

Supersedes: none. Superseded by: none.

## Context

Platform authorization is separate from tenant authorization and checks the
exact actor and action tuple. `AuthorizedPlatformAction` prevents external
construction but carries its action only as a runtime enum. Each protected
platform use case repeats a wrong-action check despite having a fixed required
action. Routing evidence to the wrong use case therefore fails at runtime.

ADR 0071 made the corresponding tenant evidence action-typed. The four
persisted platform actions and their policy adapters already have exact
semantics. This decision should preserve them.

## Decision

`fleetiq-access` owns sealed zero-sized platform action markers for
`ManageIdentity`, `ManagePlatformAccess`, `ProvisionTenant`, and
`ManageTenantLifecycle`. Each maps to one existing `PlatformAction` variant.
`PlatformAuthorizationService::authorize::<A>` evaluates the current policy
for that variant and returns privately constructed `AuthorizedPlatform<A>`
only on an allow decision.

Platform use cases require their exact evidence type. Remove unreachable
wrong-action error variants and runtime checks while preserving policy denial,
policy failure, single-use ownership, and repository failures. Compile-fail
documentation checks incompatible action routing and external construction.

## Consequences

Passing a tenant-provisioning grant to identity administration or a grant
administration method is a compile-time error. The public Rust API changes:
callers select a marker when authorizing and supply the resulting exact type
to the use case. `fleetiq-application` temporarily re-exports the new types
while capability consumers migrate.

The persisted action vocabulary, grant rows, authentication flow, and runtime
policy decisions are unchanged. No database migration or deployment change is
required. Rollback is source-level only.

## Alternatives considered

Keeping untyped evidence would retain repetitive checks and a routing error
that the compiler can prevent. Runtime conversion from untyped to typed
evidence would merely relocate the same error. Public marker implementations
would allow caller-defined actions outside the closed policy vocabulary.
