# ADR 0155: Expose a one-shot numeric summary command

## Status

Proposed, 8 November 2026.

## Context

Numeric summaries have a protected read API and a bounded trusted producer.
The installed binary needs an explicit way for an operator to publish a
summary for a known tenant and exact history selection.

## Decision

Expose `fleetiq-server summarize-once` as a trusted operator command. Read
the target from the process environment and validate it before connecting.
Check the installed schema without migrating. Reuse the existing producer,
including tenant admission, the turn deadline, verified restore selection and
immutable publication. Return the series and revision so exact retries can
be reconciled. Keep failure output bounded and redacted.

## Consequences

Operators need private database access and an explicit target. The existing
summary HTTP read contract stays compatible. Ordinary server startup adds
no producer or scheduler. Installed qualification uses a reviewed backend
publication containing the command.

## Alternatives considered

A mutating HTTP route would require separate browser authority, workload
admission and request ownership decisions. A recurring worker would require
a durable scheduling contract. A one-shot process meets the current operator
need using the existing trust boundary.

## Verification

Executable tests cover invalid targets, disabled policy, denied admission
and unavailable restore coverage. A migrated PostgreSQL test invokes the
binary twice and reads the same exact revision through the HTTP summary API.
