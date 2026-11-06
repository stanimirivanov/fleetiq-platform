# ADR 0145: Restore archived evidence into bounded staging

## Status

Proposed, 6 November 2026.

## Context

Historical reads need an explicit verified restore before hot payload reclamation.
Restore must preserve exact original evidence without replaying ingestion,
re-resolving metadata, or depending on retained hot payload values.

## Decision

History owns generation-bound restore tickets, attempt leases, availability states
and the verifying-source/publication ports. The archive adapter retrieves the
manifest's object through existing whole-file length/SHA-256/tenant verification,
decodes the existing bounded Parquet profile, and rejects more than sixteen rows.
PostgreSQL stages the exact verified file, capped at 2 MiB, rather than introducing
a second evidence serialization or rebuilding rows from current hot sources.
Later bounded query consumers decode that file independently of hot payloads.

A trusted caller explicitly requests one published manifest. Each tenant/manifest
has one restore slot. Initial expected generation zero creates request one;
exact retries return the same ticket without changing state or lifetime. A new
request after cancellation/expiry must name the prior request generation. Delayed
requests, cancellations and completions cannot reopen or expose another generation.
Tickets and descriptors are validated values, not authorization capabilities;
callers establish current tenant authority before invoking these internal ports.

Requests expire after one hour, including pending work. Claims require at least
twenty seconds of remaining request lifetime and use thirty-second attempt leases
and the existing tenant work reservations. Explicit server restore_once composition
has a twenty-second deadline; statements have two-second deadlines. No HTTP route,
polling worker, automatic startup or deployment input is added in this slice.

Admission reserves the known complete-object length. Live staged file bytes plus
active work reservations share the existing tenant max_bytes ceiling. This also
reduces available admission for existing history reads/archive work while staging
is live; existing admission-denied responses remain unchanged. Tenant-row locks
serialize policy changes, admission and staging publication. Lowering a policy
below existing live usage denies new admissions rather than deleting existing
copies. Disabling policy fences pending completion, not previously verified reads.

Publication checks active tenant, request/attempt generation, unexpired request,
lease/reservation and current policy revision. A set-wise comparison matches every
decoded interpretation identity against immutable manifest membership, including
binding revision and time basis; matching row count alone is insufficient. The
complete file's length and SHA-256 must match the manifest. Bytes, ready state and
reservation release commit atomically. Exact completion retries converge without
replacing evidence. No partial staging becomes readable.

Cancellation immediately hides/removes the temporary copy. It does not free work
that may still run elsewhere: its reservation remains until the attempt stops or
expires. Failed verification abandons only its own attempt/reservation. Unknown
outcomes and interrupted workers recover by lease expiry. Reads bind to tenant and
request generation and require ready/unexpired state and active tenant; missing,
pending, cancelled and expired states never imply complete empty history.

Expiry makes a copy unreadable immediately by database time. Explicit cleanup and
subsequent claims clear at most sixteen expired copies/requests per turn, preserving
request identity receipts. Same-generation TTL extension is rejected; shortening
TTL is conservative revocation. Live bytes are logical file bytes, not PostgreSQL
physical/TOAST allocation. Expired physical cleanup is eventual; no disk-capacity
or scheduled-cleanup claim is made before worker packaging lands.

## Consequences

Apply additive migration 0055 before using a compatible binary; shared admission
now reads restore staging. The existing schema checker requires a coordinated
quiescent schema/binary cutover rather than mixed-version rolling compatibility.
Operational rollback stops restore callers on a post-0055 compatible binary and
expires/cancels temporary copies. Keep manifests, objects, provenance and hot rows;
there is no down migration or original-evidence deletion authority.

The fixed one-hour TTL, one-manifest request scope and sixteen-row file profile
are explicit initial bounds. History HTTP/query coverage and cursor integration
belong to M06-PR8; retention, packaging, production provider durability and retained-
volume qualification remain later work. No dependency or dependency exception is
added; integration tests reuse the existing pinned S3 fixture and its reviewed
server test-only dependency ownership.

## Alternatives considered

- Replay or reconstruct from current hot rows: rejected; restore must retain
  original exact values, times, identities, quality, units and provenance.
- A second JSON evidence format: rejected; exact verified file staging reuses the
  established codec, avoids representation drift and stays independently bounded.
- Publish chunks incrementally: rejected; partial staging cannot claim availability.
- Free reservations on external cancellation: rejected; another process may still
  be finishing its bounded object read.

## Verification

Real PostgreSQL tests cover cancellation, exact request/completion retry, stale
attempts, tenant/policy fences, membership mismatch, corrupt bytes, live-copy work
budgets, expiry and populated 54-to-55 upgrade. Real PostgreSQL/S3 qualification
rejects corrupted objects, cancels a paused-provider read, retries after expiry,
compares exact original bytes/decoded evidence, and verifies archive survival after
staging expiry. Full acceptance and final-commit supply-chain checks are required.
