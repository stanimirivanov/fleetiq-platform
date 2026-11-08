# ADR 0158: Expose bounded history expiry cleanup

## Status

Proposed, 8 November 2026.

## Context

Request-driven cleanup can leave expired restore staging and export payloads
in PostgreSQL indefinitely when tenants stop making history requests.

## Decision

Expose a trusted, one-shot server command that takes one explicit tenant and
runs the existing PostgreSQL expiry routines with their existing batch limits
and lease fences. Report counts only after both routines return successfully.
Operators schedule the command independently of request traffic.

## Consequences

Restore and export cleanup commit independently. If the second batch fails,
retrying the same tenant is safe. Cleanup releases logical payload storage
while retaining job receipts and published archive objects.

## Alternatives considered

A resident scheduler would need tenant discovery, cadence configuration, and
ownership across replicas. An explicit command lets the operator set cadence
and tenant selection using existing job infrastructure.

## Verification

The executable integration test uses PostgreSQL to prove cleanup for an idle
tenant, batch limits, retry behavior, and preservation of live leases and
retained identities.
