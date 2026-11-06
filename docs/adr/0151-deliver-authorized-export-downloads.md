# ADR 0151: Deliver authorized export downloads

## Status

Proposed, 6 November 2026.

## Context

ADR 0150 provides bounded durable export selections and exact temporary artifacts.
Operators need protected creation, status, cancellation and download without
exposing storage credentials or making object URLs public. Slow/abandoned delivery
must not escape its own resource envelope or exhaust tenant admission silently.

## Decision

Add HTTP baseline.35 under the existing listener-selected workload/browser identity.
PUT /exports/requests/{request_key} creates/retries an explicit selection.
GET/DELETE /exports/jobs/{export_id} inspect/cancel it; GET/HEAD on its /download
resource deliver attachment bytes or metadata. Paths include the authorized tenant.

Every operation consumes current telemetry.read evidence. Creation/cancellation
also consume independent operations.manage for the same tenant and actor and use
the listener-selected browser mutation guard. No new action vocabulary or implicit
grant is introduced. Reject identity changes between the two authentication lookups;
never combine one actor's telemetry grant with another actor's operational grant.

Creation accepts a closed JSON command of at most 4096 bytes and inherits the
sixteen-row, seven-day, exact Parquet/profile and one-hour lifetime limits of
ADR 0150. Eight shared capture/worker slots bound expensive demand-driven turns.
An accepted pending selection starts one explicit twenty-second worker attempt.
Retrying the same request key can resume pending work; there is no startup loop,
automatic polling scheduler or reselection on retry. A process interruption leaves
the durable job available to the existing trusted worker or an authorized retry.

Downloads consume one additional tenant slot and 2 MiB reservation plus one of
eight global transfer slots. Policy must be enabled and admit the transfer; status
and cancellation do not need a new history reservation. A single-slot policy can
produce an export but must be enlarged before serving a concurrent download.
HEAD rechecks authority and ready/expiry state and returns only metadata, without
loading a file or reserving transfer capacity.

GET loads one verified complete file, at most 2 MiB, and emits independent frames
of at most 64 KiB. The five-second total preparation/body deadline is also limited
by artifact expiry. A watchdog can free the full source buffer and global permit
even when the transport does not poll the body. The permit remains reachable during
final-frame reservation release. That release is independently polled under the
same deadline; the final frame rechecks expiry after it completes. Stream/drop
ownership aborts unused watchdogs. A stalled frame cannot retain the full file's
allocation through a shared byte slice.

A successful transfer acknowledges its durable reservation before the final frame.
Dropped/timed-out/uncertain transfers conservatively retain it until its original
thirty-second expiry, reconciled by existing admission. Bytes already delivered
cannot be recalled; authority is per request. New requests after revocation fail.
Cancellation removes later download availability but cannot recall in-flight bytes.

Responses use no-store, nosniff, exact Content-Length, application/vnd.apache.parquet,
and a generated attachment filename containing only the validated numeric export ID.
No public object URL, redirect, user-controlled filename or storage credential is
returned. Range/resume is unsupported and explicitly returns 416 for GET. Expired/
cancelled jobs return 410, pending downloads 409 and missing scoped identities 404.
Unknown query parameters and malformed/forged request context fail explicitly.

## Consequences

The API is additive. Regenerate clients before consuming baseline.35. No migration,
format, dependency or permission vocabulary changes. Bounded job metadata exposes
identity, effective state, selected generation, format, original expiry, row/file
counts and selected coverage without reading private frozen evidence arrays.

Capture/worker and transport capacities are separate. A partial transfer may hold
its short tenant allowance after disconnect, but cannot keep the full buffer/global
slot indefinitely. Limits describe bounded logical work, not measured production
RSS, network throughput or installed capacity. Public UI/deployment qualification
remain later work.

Rollback removes these routes/callers and stops demand-driven work; existing
fenced attempts/reservations expire through ADR 0150's lifecycle. Preserve durable
job identities, frozen evidence and previously published migrations.

## Alternatives considered

- Public or signed object URLs: bypass per-request application revocation checks.
- Browser storage credentials: expose infrastructure authority to the client.
- Implicit HEAD through GET: loads files and leaves conservative download reservations.
- One shared backing allocation for all frames: a stalled frame retains the whole file.
- Releasing admission on any disconnect: treats uncertain work as already stopped.
- Separate newly introduced export actions: existing independent operational and
  telemetry grants already express the required command/data boundary.

## Verification

Real PostgreSQL HTTP tests cover lifecycle, exact values, current grants, same-actor
composition, browser mutation checks, cross-tenant access, HEAD, ranges, expiry,
partial transfer and global capacity. Paused-clock tests cover unpolled/partial
bodies and blocked final-frame cleanup. Actual response metadata is validated
against the generated OpenAPI schema. Full acceptance, OpenAPI and final-commit
supply-chain checks gate the PR.
