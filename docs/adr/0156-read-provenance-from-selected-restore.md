# ADR 0156: Read provenance from a selected restore

## Status

Proposed, 8 November 2026.

## Context

A restored history page can show reclaimed interpretations. The existing exact
interpretation lookup reads hot evidence and returns 409 after reclamation.
The inspector needs an exact, bounded lookup within the selected restore.

## Decision

Baseline.37 adds paired manifest and restore generation query parameters to
the existing interpretation route. Authorize current telemetry access before
processing the selection. Read the existing verified staging snapshot and
match the exact interpretation identity and property target. The selected
read has no hot fallback. Return 404 for unavailable staging or a nonmember
interpretation. Keep hot-only behavior when both parameters are absent.

## Consequences

Clients can regenerate and carry the exact restore scope into inspection.
There is no persisted format change or new authority. An old client can
continue reading hot provenance using its original request.

## Alternatives considered

An implicit search across restores would make the requested evidence and
resource use ambiguous. A separate endpoint would duplicate the target and
response contracts of the existing exact lookup.

## Verification

A real PostgreSQL route test compares reclaimed and restored provenance,
rejects invalid selections, and covers scope, expiry and revocation.
