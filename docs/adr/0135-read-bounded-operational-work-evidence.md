# ADR 0135: Read bounded operational work evidence

## Status

Proposed, 31 October 2026.

Supersedes: none. Superseded by: none.

## Context

Operational work spans replay, calculation, alert evaluation and notification
positions. Existing device diagnostics require protocol.manage and do not imply
cross-capability operational authority. An empty queue cannot establish health.

## Decision

Add independent tenant operations.read, explicitly delegated through existing
administration/discovery. No old grant gains authority. Baseline.27 adds a tenth
action and one no-store work-diagnostics GET. Strict consumers regenerate before
delegation. Preserve the existing device diagnostics protocol.manage contract.

Sample at most 64 tenant identities per family plus one coverage sentinel.
Exclude completed replay/calculator work. Alert evaluation samples selected rules
and their next exact-selector evidence through the existing worker function.
Notification sampling covers retained destination/incident positions, including
refresh/reconciliation history. Never return identity keys, endpoint digests,
requests, values, payloads or raw failures. Closed categories distinguish current
leases, eligibility, blocking, deferred retries and exhaustion. Report the oldest
sampled eligible evidence/due timestamp, never a global oldest or total backlog.
Truncation concerns positions/rules; each rule contributes at most its next input.
Worker liveness and destination seeding remain not_observed even for empty samples.

Use one repeatable-read read-only snapshot, a two-second statement limit and a
five-second HTTP storage-turn timeout. Tenant-leading indexes support deterministic
prefix seeks. Existing exact alert selector cost is bounded by deadline and a
maximum of 65 rule calls; this is not constant-time or production load qualification.
Native evidence belongs to analytics; SQL to PostgreSQL; HTTP mapping to server.
No new dependency, worker state, metrics service or recovery command is introduced.

## Consequences

Migration 0050 expands small grant/audit checks without changing rows or grants.
New index builds scan retained tables under ordinary transactional index-build
locks; qualify migration duration and write blocking against installed data.
Grant runtime SELECT on work, selector and supporting identity/metadata sources;
no DDL or privileged fallback. Roll out migrated compatible binaries before
explicit delegation. Roll back the route/grants through a compatible forward
build; old binaries cannot accept the new migration ledger/action vocabulary.

Samples are conservative observations, not complete monitoring. Old notification
positions can dominate a prefix; truncation does not establish that no other
failures exist. PR18 adds audited recovery and PR19 presents these limits in UI.

## Alternatives considered

Reusing metadata, protocol or alert management would silently widen authority.
Global counts/oldest scans would make cost proportional to history. Independent
metrics infrastructure would duplicate retained state and exceed this slice.
Adapt the existing bounded ingestion diagnostic pattern and current worker state
instead; introduce indexed monitoring aggregates only with separate scale evidence.
