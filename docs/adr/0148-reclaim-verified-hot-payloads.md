# ADR 0148: Reclaim verified hot payloads

## Status

Proposed, 6 November 2026.

Extends ADR 0147.

## Context

The approved M06-PR10 requires bounded, verified hot reclamation while retaining
identity, deduplication and provenance. One capability includes the executor and necessary compatibility changes. Three hot projection timestamps were estimated
by PR9; resolution cutoffs cannot be reconstructed as original facts from today's
metadata. Current projection pointers and queued work still depend on hot rows.

## Decision

Keep every interpretation key and its source/binding foreign keys in
history.asset_property_changes. Permit only a one-way, all-or-none NULL transition
of event_at_ms, resolution_effective_at_ms and resolution_known_at_ms. The exact
original fields remain in the verified immutable archive. Original wire, decoded
values, binding revisions, manifest membership and lineage/deduplication anchors
remain unchanged. New inserts must always supply complete facts.

No startup worker, HTTP command or policy configuration enables reclamation.
A separate private trusted operator approval binds tenant, current policy revision
and active actor. Approval requires human deployment review; this implementation
does not grant operations.read destructive authority. Revocation removes only the
approval. Preview contexts are not accepted by the executor.

One explicit turn selects one published manifest (at most sixteen identities,
2 MiB), reserves a conservative 2 MiB tenant work allowance, reads and verifies
complete provider bytes through the existing bounded decoder, then compares exact
original evidence with the still-hot members. Corrupt/missing objects, foreign
tenants, incomplete membership or changed provenance fail closed. Provider and
database work share a twenty-second deadline; cancellation leaves budget capacity
reserved until the existing thirty-second expiry.

Before mutation, an explicitly READ COMMITTED database transaction excludes writes to the history and
known dependency relations. It checks tenant/actor, exact current approved policy,
verified immutable publication, conservative receipt age, unfinished replay,
semantic discovery/work, calculator schedules/work/preparations, restore work,
pending projections, exact current projection pointers and terminal calculator
evaluations referencing a selected identity. It then inserts an
immutable receipt and clears all manifest payload fields atomically. Receipt
insertion also enforces protections, and a deferred database trigger rejects a
receipt without its complete matching transition. Ordinary updates, anchor edits,
row deletion, partial clearing and rehydration remain forbidden.

The private application database role and verifying host remain trusted, as for
existing archive publication. This is not a security boundary against the database
owner fabricating an archive or disabling database protections. No public database
or tenant role is introduced.

Coarse relation locks intentionally trade throughput for safe absence checks,
including new consumer creation during verification. Direct receipt transactions
at stronger/stale-capable isolation are rejected. Calculator and projection writers
lock referenced history rows, making stale repeatable-read references abort rather
than publish pointers to reclaimed payloads. Each statement/lock wait has
two seconds. Timeout/deadlock/cancellation rolls back and leaves the manifest
retryable. A committed receipt reconciles the exact original target without a new
provider read. This is not a claim of atomicity with a hostile storage provider:
deployment requires immutable retained objects and independently qualified storage.

Legacy hot history fails closed with archived-history 409 if any interpretation
for that exact target has reclaimed payload. This conservative target-level check
and the page share one repeatable-read snapshot. Exact provenance also returns
409, including after temporary restore; callers select the manifest/generation
through hot_and_restored history to obtain original evidence. Selected coverage
remains partial, never whole-interval completeness.

Source-based semantic retries still validate immutable durable lineage, recognize
the retained identity and never refill removed timestamps. Existing first-cutoff
retry semantics remain unchanged. Projection rebuild excludes archived losers;
current winners and pending work are protected before reclamation. New calculator
preparations reject targets with missing hot evidence rather than computing from
an incomplete empty input. Previously prepared terminal snapshots remain intact; their referenced identities
are pinned. An additional database guard rejects new evaluation references to
archived payloads, preventing SQL NULL comparisons from accepting forged inputs.

## Consequences

Migration 0056 relaxes only payload presence and adds private approval/audit state.
The all-or-none CHECK is installed NOT VALID because existing rows already satisfy
three NOT NULL constraints. It enforces future writes immediately without scanning
history under an exclusive lock. Schedule separate validation if needed. DDL still
requires exclusive lock acquisition; use lock_timeout and quiesce writers. No row
rewrite/backfill is required.
Deploy compatible binaries before approval; old binaries cannot safely read
reclaimed payloads. Logical removed field bytes are records times 24, not net,
physical, index, TOAST, billing or vacuum savings. Retained anchors and new audit
records may outweigh savings for these small payloads. Measure before enabling.

The public baseline.33 adds explicit archived-history responses, projection and
already-archived preview protections and the trusted reclamation capability
indicator. No new dependency, action, storage endpoint or credential is introduced.

Rollback must first revoke approval and stop callers. Reverting code does not
restore hot fields. Query exact original evidence through verified temporary
restore with compatible binaries; do not bypass immutable guards to rehydrate.
Tenant-wide holds, coarse locks and target-level read refusal can overretain or
deny otherwise answerable work. Packaging, production/provider certification and
retained-volume qualification remain later M06/M08 work.

## Alternatives considered

- Delete semantic rows: breaks foreign-key and idempotency anchors.
- Drop/disable immutable triggers: grants an uncontrolled mutation path.
- Trust a preview or event timestamp: stale dependencies and late arrivals are unsafe.
- Reconstruct original resolution cutoffs: would change historical attribution.
- Partition retrofit or source-value tiering: separate reference graphs outside this PR.
- Optimistic absence checks alone: do not exclude a concurrently inserted consumer.

## Verification

Real PostgreSQL tests enforce default-off behavior, immutable anchors, receipt
completion, exact retries, current rebuilds, fresh policy/restore/calculator holds,
concurrent execution and dependency-write exclusion. Real S3 tests reject corrupt
bytes and restore original provenance exactly. HTTP tests verify archived 409,
selected restored coverage and authority revocation. A guard-removal mutation must
fail its safety assertion. Full acceptance and final-commit supply-chain gates are
required before PR creation.

The shared budget allocator also pins READ COMMITTED before waiting on the tenant
lock. Otherwise two repeatable-read callers could both preserve a pre-admission
snapshot and exceed the slot limit. The receipt retains its admission ID and
checks that exact policy-bound reservation again at commit. Stale first calculator
preparations are rejected; the normal preparation adapter explicitly selects the
supported isolation.

Baseline.33 requires client regeneration for retention-preview validators: its
capability constant changes from false to true and protection enums expand.
Hot-only history/provenance gains 409 only after explicit reclamation. No existing
hot payload is changed merely by deploying compatible binaries.
