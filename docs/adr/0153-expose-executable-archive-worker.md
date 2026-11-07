# ADR 0153: Expose the executable archive worker

## Status

Proposed, 7 November 2026.

## Context

The archive publication functions can process one bounded durable selection.
The local Compose package needs an executable mode with explicit ownership,
storage prerequisites and graceful stop.

## Decision

The `fleetiq-server archive-worker` command requires
`FLEETIQ_ARCHIVE_TENANT_ID`, `DATABASE_URL` and all five existing archive
storage inputs. Those inputs are `FLEETIQ_ARCHIVE_ENDPOINT`,
`FLEETIQ_ARCHIVE_BUCKET`, `FLEETIQ_ARCHIVE_REGION`,
`FLEETIQ_ARCHIVE_ACCESS_KEY` and `FLEETIQ_ARCHIVE_SECRET_KEY`.
Deployment supplies credentials through its private mounted inputs.

Startup opens a small database pool and validates the existing migration
inventory. Each turn uses the same verified publication logic and twenty-second
deadline as the trusted one-shot command. The worker pauses one second between
turns. Tenant lifecycle policy, work admission and generation fences remain the
authority for claiming and publishing.

SIGTERM or Ctrl+C cancels the current turn. The frozen selection remains durable
and can be reclaimed for another attempt after its lease expires. Ordinary
server startup never starts the worker. The worker publishes archive manifests.
Reclamation, restore and export have separate ownership and activation.

## Consequences

Deployments must migrate before starting this process. A producer image for the
merged change is required before Compose qualification. Startup and retry
diagnostics carry no connection values, object keys or credentials.

## Alternatives considered

Embedding archive scheduling in the HTTP server would couple archive resource
ownership to ordinary startup. A separate executable would duplicate the
existing server packaging and configuration. An explicit command reuses the
binary while giving Compose an independent lifecycle.

## Verification

A real PostgreSQL and S3 test interrupts the executable while the object
provider is paused, restarts after lease expiry, and verifies publication of the
same selection with its hot payloads intact. A separate executable test rejects
missing storage configuration before connecting.
