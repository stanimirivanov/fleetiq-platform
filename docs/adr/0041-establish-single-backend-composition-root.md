# ADR 0041: Establish a single backend composition root

## Status

Accepted, 18 September 2026.

Supersedes: none. Superseded by: none.

## Context

FleetIQ has domain, application, protocol, semantic, and PostgreSQL libraries,
but no process composes them into an operable backend. The first M02 capability
needs a stable place to own process configuration, infrastructure startup,
HTTP routing, health reporting, and shutdown. Splitting these responsibilities
across independently deployed services before their scaling and ownership
requirements are known would increase operational cost and make transactional
boundaries harder to change.

The choice of HTTP framework and startup ownership affects future application
adapters and deployment topology, so it is recorded before product routes are
added.

## Decision

FleetIQ starts as one modular backend executable in the `fleetiq-server` crate.
The executable is a composition root: policy remains in the existing domain
and application crates, while infrastructure adapters are wired at the outer
boundary.

The process uses Tokio as its asynchronous runtime and Axum 0.8 for HTTP
routing. Startup validates `DATABASE_URL` and `FLEETIQ_BIND_ADDRESS`, connects
to PostgreSQL, and applies the embedded migration chain before binding the
listener. The bind address defaults to `127.0.0.1:8080`, so an unconfigured
development process is not exposed beyond the local host.

The unauthenticated operational surface contains:

- `GET /health/live`, which returns `204 No Content` when the process can serve
  HTTP; and
- `GET /health/ready`, which returns `204 No Content` after a successful
  PostgreSQL round trip and `503 Service Unavailable` otherwise.

Health responses disclose no configuration, dependency error, tenant data, or
database detail. Receiving `Ctrl+C` begins graceful HTTP shutdown, after which
the process closes its database pool. Configuration diagnostics must not expose
the database URL because it can contain credentials.

The initial runtime applies migrations itself because it is a single-process
development foundation. Production rollout and migration orchestration must be
decided before horizontally scaled deployment.

## Consequences

The repository gains a runnable backend and a clear place to compose later
metadata and administrative routes. A single process preserves cheap in-memory
composition and leaves service extraction available when measured scaling,
failure isolation, or team ownership requires it.

Axum becomes a foundational transport dependency at the outer boundary. HTTP
handlers must translate into application contracts rather than absorb domain
policy. Database availability controls readiness but not liveness, allowing an
orchestrator to distinguish a running process from one that should receive
traffic.

Automatic startup migrations simplify the current developer workflow but are
not yet a production deployment policy. Authentication, authorization, TLS,
structured telemetry, business APIs, service-level objectives, and platform-
specific probe configuration remain separate changes.

## Alternatives considered

Deploying separate services for metadata, ingestion, and analytics was rejected
because current requirements do not establish independent scaling, ownership,
or availability needs that offset the operational cost.

Using Hyper directly was rejected because FleetIQ needs ordinary routing and
state extraction rather than a custom HTTP framework. Axum supplies those
features while keeping handlers explicit and based on Tokio and Tower types.

Deferring the executable until a business API was rejected because executable
startup, configuration, migration, health, and shutdown behavior are themselves
the minimum operable foundation for M02 and can be verified independently.

Requiring a separately invoked migration command remains a viable production
deployment model. It is deferred until the deployment topology and rollout
requirements are defined.
