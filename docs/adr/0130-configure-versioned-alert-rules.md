# ADR 0130: Configure immutable numeric alert rules

## Status

Proposed, 30 October 2026.

Supersedes: none. Superseded by: none.

## Context

M05 needs a bounded configuration boundary before durable alert transitions.
Calculator evidence is immutable, tenant owned and explicitly versioned; a rule
must not silently follow a changed release, property, target or unit.

## Decision

Keep `numeric-threshold-v1` as a small typed grammar in analytics. Pin calculator
release, input scope, output target, property/version and output ordinal. The
selector matches completed outputs across execution configurations with those
exact values; it does not require a configuration or completed evaluation at
creation. Reviewed mean/MAD implementations currently publish ordinal zero.

Use exact decimal gt/gte/lt/lte comparisons in the property's canonical unit.
Require an explicit nullable threshold unit: null means dimensionless. Bound
threshold representation and precision, and event-time hold/false-resolution
durations to seven days. Missing, failed or unusable evidence is unknown, never
false. A pure predicate exposes that distinction without advancing lifecycle.
Adapt Prometheus's condition/hold-duration pattern without PromQL, a dependency,
or its wall-clock scheduling semantics. Alert execution and delivery remain later
slices; selecting a rule here does not produce alerts.

Persist immutable sequential revisions separately from optimistic activation.
Rules start inactive. Appending a revision cannot select it. Same-author exact
retries retain original receipts and never restore an earlier selection.
Serialize head mutations and recheck current authority inside each transaction.
Use tenant-qualified references and database validation for exact output/unit
compatibility, including direct trusted SQL writes.

Expose protected exact head/revision reads and revision/activation commands.
Require independent `alerts.read` and `alerts.manage` capabilities, with existing
listener authentication, browser Origin/CSRF checks and bounded JSON bodies.
Neither telemetry, metadata nor analytics administration implies these grants.

## Consequences

Migration 0045 is additive and creates no grants or active rules. The closed
HTTP action vocabulary expands from six to eight; regenerate clients before
new capabilities are delegated. Upgrade checked-schema binaries together.
Retain schema/history on rollback; older checked-schema binaries cannot run
against the newer migration ledger. No new dependency is introduced.

The bounded selector deliberately does not pin execution configuration identity.
A future need to distinguish otherwise identical configurations requires another
reviewed grammar version rather than changing existing rule meaning. No arbitrary
expressions, automatic grant migration, state transitions, evaluation worker,
notification delivery or UI authoring are introduced.

## Alternatives considered

PromQL or a general expression engine adds syntax, execution and resource policy
without a current requirement. Mutable rules would reinterpret historical alert
evidence. Automatic activation on creation would combine review and operational
selection. Reusing analytics.manage would give calculator operators unrelated
alert authority.
