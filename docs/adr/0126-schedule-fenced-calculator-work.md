# ADR 0126: Schedule generation-fenced calculator work

## Status

Proposed, 30 October 2026.

Supersedes: none. Superseded by: none.

## Context

[ADR 0125](0125-execute-pinned-window-means.md) retains prepared inputs across
retries. Background execution also needs durable window identity, bounded catch-up
and protection against a worker completing after its lease expires.

## Decision

An explicit checked-schema `analytics-worker` process installs an optional trusted
schedule file and processes active tenants' fixed windows. Immutable schedules pin
release, configuration, scope, target, first start, window, lateness and minimum.
Database time determines eligibility; a locked cursor enumerates at most 32 windows
per turn. Cursor advancement and work insertion share one statement. Windows are
half-open and known-at is their finalization time. Evaluation IDs derive from the
tenant, schedule and start; first processing time is retained in immutable work.

Claims use PostgreSQL row locks with SKIP LOCKED, 30-second database-clock leases
and increasing generations, capped at three attempts. Preparation remains durable
outside the terminal transaction. Retries reuse the complete original request and
selection. The completion transaction locks work and tenant, validates the current
active unexpired claim, appends the existing terminal envelope and rechecks expiry
when marking work done. Any failure rolls back outputs and checkpoint together.
Expired third attempts become retained exhausted work. Stale workers cannot publish.

## Consequences

Migration 0041 adds tenant-owned schedules, monotone cursors and fenced work;
there are no dependency additions. Existing one-shot execution is unchanged.
The worker is opt-in and never migrates. Each turn has a 15-second deadline,
5-second statements and a 250-millisecond pause. Restart preserves pending work.
Stopping the worker is rollback; do not remove retained schema or replay exhausted
work manually. HTTP activation, schedule changes, cron, operator reset controls and
production deployment remain later milestone slices. The trusted file is an
installation boundary, not an authorization API or a tenant isolation credential.

## Alternatives considered

An in-memory timer loses cursors on restart. An external scheduler adds a service
without replacing database fencing. Holding a transaction during calculation
prevents durable preparation reuse after process loss. A lease check only before
append can admit a result after expiration; completion must check again.
