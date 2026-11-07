# ADR 0154: Expose a bounded hot-reclamation command

## Status

Proposed, 7 November 2026.

## Context

Installed lifecycle qualification needs to exercise verified hot reclamation
through the packaged backend. The trusted Rust entry point already enforces
approval, admission, object verification and atomic retention receipts.

## Decision

Expose one explicit `fleetiq-server reclaim-once` invocation for a tenant,
manifest ID and expected policy revision. Read the target and existing database
and archive settings from the process environment. Validate the installed schema
in check mode and invoke the existing bounded executor. The command emits one
closed outcome and only redacted errors.

Approval and revocation remain separate trusted operator actions. Ordinary
startup and archive publication do not invoke the command. The existing
retention transaction owns all authorization and mutation checks.

## Consequences

The command runs with operator-owned database and private storage access. An
installation must migrate before invocation. Callers can reconcile an uncertain
result by retrying the same target. Published HTTP contracts and persisted
formats are unchanged.

## Alternatives considered

A qualification-only SQL mutation would bypass archive verification, approval
and receipts. An HTTP endpoint or recurring worker would expand activation and
authorization beyond the single trusted turn the installation needs.

## Verification

Executable tests cover redacted input validation. PostgreSQL and object-provider
integration verifies disabled approval, successful reclamation and exact retry
after the archive object is corrupted.
