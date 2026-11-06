# ADR 0150: Produce bounded historical exports

## Status

Proposed, 6 November 2026.

## Context

M06 needs asynchronous exports whose membership cannot drift across restart,
late interpretation, restore expiry or attempt retry. Existing property history,
verified restored staging, exact Parquet and tenant work admission supply the
boundaries. Raw history paging alone is not a durable snapshot.

## Decision

Add explicit trusted request_export and export_once compositions, each with a
twenty-second deadline. No HTTP/download route, scheduler or startup worker is
added. Callers establish tenant authority. A validated tenant-scoped request key
identifies one exact property-version/window/optional restore-generation scope.
Exact retries retain the original ticket and terminal state; another selection
needs another key. Scope collisions fail instead of silently replacing evidence.

Capture hot rows and an optional ready verified restored file in one repeatable-read
snapshot. Verify complete selected membership, tenant ownership and exact hot/restored
overlap before deduplication. Known reclaimed target evidence outside the selected
manifest fails coverage conservatively. Reject more than sixteen unique inputs,
empty selections, oversized fields/files and invalid evidence without truncation.
Windows are half-open, nonempty and at most seven days. Order the frozen selection
by event time then canonical complete identity JSON bytes; no correction collapse,
unit conversion or reevaluation occurs.

Reuse property-history-v1 Parquet with its existing exact fields and codec envelope:
at most 8 KiB per field, 1 MiB logical fields and 2 MiB per file. The request serializes
the selected generation once into durable bounded staging. Asynchronous workers
consume that file, decode/re-encode/verify identical complete bytes and publish ready
availability. They never re-query current sources. The selected generation and format
version remain one; worker attempts have an independent increasing fence.

Temporary source/artifact bytes share one database-owned lifecycle, following restore
staging. This avoids an external object-store commit/orphan protocol for temporary
exports. Pending and ready jobs last at most one hour and consume one shared tenant
slot plus a conservative 4 MiB work/staging charge. Creation atomically transfers its
short preflight reservation into this job charge. Ordinary history admission and
restore quota checks include exports. Worker claims reuse the already-held job slot,
so a tenant limited to one job can complete it without a second slot.

Tenant locks and READ COMMITTED publication serialize current-policy quota decisions.
A thirty-second attempt lease fences publication. Cancellation/expiry remove artifact
availability and bytes while retaining any running lease charge until stop acknowledgement
or expiry. Cleanup is explicit and bounded to sixteen rows per class per call. Reads
check active tenant, exact ticket, ready state, expiry, length and digest. Artifact
metadata preserves selected generation, format, original expiry and restored scope.

SQL guards freeze scope, membership file, digest, generation and bounded lifetime,
fence attempt changes/publication, prevent terminal reopening and reconcile the
one-for-one reservation transfer at commit. Trusted private writers keep completion
constraints deferred until commit. The private database owner and codec host remain
trusted for evidence authenticity; no SQL guard can prove that a host stopped executing.

## Consequences

Migration 0058 is additive, with no source backfill or raw-history mutation. Shared
quota queries change because exports must compete with other history work. Live/running
job counts use expiry/lease indexes so permanent lightweight identity receipts do not
require scanning every prior tenant export. The four-MiB allowance is conservative
logical admission, not a physical-storage or measured-RSS claim.

A request performs bounded source materialization; expensive arbitrary queries and
unbounded streaming are unsupported. Asynchronous workers publish verified artifacts
from that fixed selection. A frozen file remains meaningful after staging expires or
hot evidence is reclaimed. Coverage describes available hot/selected-manifest evidence,
never interval completeness. New keys capture late evidence; retries never do.

There are no new crates, packages, features or formats. Storage accounting does not
silently classify these copies as measured physical usage. Rollback stops export callers
and reconciles live copies/leases before running a version whose shared admission does
not count exports; retain the additive schema and identity receipts.

## Alternatives considered

- Re-querying pages after restart: changes selected membership and loses snapshot meaning.
- CSV or a new wire schema: adds precision/escaping policy already solved by exact Parquet.
- Permanent object-store export publication: needs a separate lifecycle and orphan protocol;
  outside this bounded temporary artifact slice.
- Releasing capacity immediately on cancellation: admits overlapping work while the owner
  may still execute.
- Unlimited files or silent row truncation: violates explicit resource and evidence bounds.

## Verification

Tests cover exact original values, frozen late-arrival membership, restart and stale
attempts, restored overlap/expiry, row/file overflow, current policy, separate shared
slot/byte limits, cancellation, durable commit fences and bounded cleanup. Complete
acceptance and final-commit supply-chain checks gate the linked PR. Installed/production
capacity qualification and public export flows remain later work.
