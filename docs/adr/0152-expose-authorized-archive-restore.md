# ADR 0152: Expose authorized archive restore

## Status

Proposed, 7 November 2026.

## Context

The archive restore repository and one-turn runner can stage a verified
manifest for exact history review. An operator needs a published HTTP
entry point before the browser can request that staging.

## Decision

Publish baseline.36. Under the tenant restore manifest resource, PUT
`requests/{expected_generation}` takes an empty body and requests the
specified manifest. Zero selects an initial request. To replace expired or
cancelled work, supply its generation. Retrying the original expectation
preserves its ticket and lifetime. GET `generations/{restore_generation}`
reports the state of that exact ticket. Both operations reject unknown
query parameters.

The listener supplies authentication and the browser mutation guard. Status
consumes current telemetry.read. Requests also consume independent
operations.manage for the same actor and tenant. The status response
contains only the manifest, generation and availability state.

Archive configuration is required for requests. A pending receipt can start
one twenty-second attempt under the listener's eight-slot semaphore. The
existing generation, lease, reservation and verified-object fences determine
whether staging can become ready. A failed attempt leaves pending work
recoverable by exact retry. Ready describes only the selected manifest.

## Consequences

The UI can select ready staging through the existing restored-history
query. No migration or grant vocabulary change is required. Installations
must supply private archive configuration and enable tenant lifecycle policy
before requesting work. Cancellation transport, manifest discovery, automatic
worker scheduling and installed-product qualification stay outside this slice.

## Alternatives considered

A browser-only command would have no authoritative way to create staging.
A startup worker would introduce deployment and scheduling changes before
the opt-in worker packaging slice. A one-turn request reuses the existing
bounded restore runner and leaves durable retry available to operators.

## Verification

A real PostgreSQL and S3 regression covers grant and browser mutation
checks, exact replay, verified staging, scope rejection, expiry and revoked
read access. The status response is checked against its published example.
