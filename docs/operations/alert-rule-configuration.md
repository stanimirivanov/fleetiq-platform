# Alert rule configuration

## TL;DR

M05 persists protected versioned numeric rules, inactive by default. This slice
configures rules only: there is no alert evaluation worker, transition history,
notification delivery or authoring UI. See [ADR 0130](../adr/0130-configure-versioned-alert-rules.md).

## Authority and commands

Use the existing listener's workload bearer or browser session. Reads require
exact tenant `alerts.read`; commands require separate `alerts.manage`. Browser
writes also require the established Origin and CSRF headers. The migration grants
neither action automatically, and analytics/metadata/telemetry grants imply neither.
Regenerate clients for HTTP baseline.24's eight-action vocabulary before delegation.

All paths start `/api/v1/tenants/{tenant_id}/alert-rules/{rule_id}`:

- GET the base path for `latest_revision` and the current activation receipt.
- PUT `/revisions/{revision}` with the closed definition to append the next
  positive u32 revision, starting at 1. GET that path reads retained content and
  original actor/time. Creation does not activate the revision.
- PUT `/activation` with `expected_generation` and required nullable `revision`.
  Start at generation 0; a positive retained revision selects it, null deactivates.

Exact same-author/content retries return the original receipt, including after
later changes. They never restore an old selection. Conflicting content, skipped
revisions, stale generations and unavailable selector references return 409.
Missing reads return 404. All responses use no-store; invalid input is 400 and
current-authority failures are 403. Grant and lifecycle state are rechecked and
locked in each write transaction.

## Definition semantics

`kind` is `numeric-threshold-v1`. Pin calculator release (`calculator`, `version`),
input `scope`, output `target`, `output_property`, `output_version` and
`output_index`. Current admitted mean/MAD releases support output ordinal 0.
The target must declare that exact decimal property. The selector spans matching
execution configurations; it does not depend on an already completed evaluation
or an active configuration.

`comparison` is gt/gte/lt/lte. Supply `threshold` as a finite decimal string, at
most 256 input characters, magnitude below 10^128 and at most 64 normalized
fractional decimal places. Equivalent thresholds normalize for retry comparison.
`threshold_unit` is required: use the exact property's canonical unit, or null
for a dimensionless output. Units are not converted.

`hold_ms` specifies matched event-time duration before firing. `resolution` is
`resolve_on_false`; `resolve_after_ms` specifies explicit false event-time duration
before resolution. Both durations range from zero through 604800000. Missing,
failed and unusable evidence is unknown rather than false. The configuration
slice does not advance timers or states; later lifecycle execution must preserve
these semantics. No arbitrary expressions are accepted.

## Migration and rollback

Apply 0045 through the normal checked-schema migration entrypoint and deploy
schema-compatible binaries together. Existing rules/grants are not imported or
activated. The migration briefly locks the grant/audit tables to expand their
closed action constraints. Retain immutable revisions and activation receipts on
rollback; do not delete history or attempt an older binary against a newer ledger.
Disable affected traffic while deploying a compatible correction.
