# Bounded historical exports

## TL;DR

- Apply migration 0058; export calls are explicit and start no worker loop.
- Choose one tenant, request key, property version, bounded window and optional ready restore.
- Configure one slot and 4 MiB per live export in existing lifecycle policy.
- Retry the same key to retain selection; use a new key for new evidence.
- Cancel/expire artifacts explicitly; uncertain running attempts hold capacity until expiry.

## Request and format

The trusted server composition request_export accepts ExportKey and ExportRequest.
The caller establishes tenant authority; this slice publishes no HTTP or download
endpoint. Keys are 1–128 ASCII lowercase letters/digits/dot/underscore/hyphen and
start with a letter or digit. They are request identities, never filesystem paths.
An existing key cannot change scope, refresh expiry or reopen a terminal request.
Concurrent requests recheck committed winners after admission/capture failure. A
winner that is still uncommitted may cause a transient denial; retry the same key.

Choose an exact asset/property-definition version and a half-open event-time window
of at most seven days. An optional RestoreTicket selects exactly one manifest and
request generation. Capture combines hot rows and verified ready staging in one
repeatable-read view. Overlap is compared by complete identity and original evidence,
then deduplicated. Missing/unready/expired staging or known reclaimed target evidence
outside the selected manifest fails Coverage. Empty or more-than-sixteen selections
fail Limit. No partial/truncated export is published.

Frozen selection generation one uses the existing property-history-v1 exact Parquet
codec. Its integer/decimal values, units, quality, attribution and complete source
identities remain exact. Codec limits are 8 KiB per field, 1 MiB logical fields and
2 MiB complete file. The request materializes this bounded selection; workers never
refresh it from live sources. Coverage means the captured hot/selected manifest
snapshot, never complete interval coverage. No correction collapse or unit conversion.

## Produce and retrieve

Call export_once for the exact tenant and ExportTicket. Each twenty-second turn
claims a thirty-second fenced attempt, decodes/re-encodes/verifies the frozen file,
and atomically makes its identical bytes ready. Restart retries the selected file
with a new attempt, not a new selection generation. Late evidence requires a new key.
A known codec/publication failure acknowledges stopped work; uncertainty retains
capacity until the lease expires. Pending selections remain available to retry until
their original one-hour expiry.

PostgresExports provides trusted status, artifact, cancel and expire operations.
Ready artifacts carry exact bytes, format version, selected generation, original
expiry and optional restored scope. Availability checks the active tenant and
expiry/digest/length every time. Expired/cancelled artifacts are never readable,
even if physical cleanup has not run. Source restore expiry does not invalidate an
already frozen export selection.

## Quotas and cleanup

An enabled current lifecycle policy must admit one job slot and a conservative
4 MiB work/staging allowance, alongside ordinary history reservations and live
restore copies. This charge covers a frozen file and bounded encode/output work;
it is not measured process memory, disk allocation or billing. No policy means no
new exports. A reduction/disablement prevents new worker claims or publication;
existing terminal/expiry cleanup remains possible.

Creation transfers its short reservation into the durable job charge atomically.
A worker uses that existing slot, allowing max_jobs=1 to finish a single export.
Pending and ready copies consume quota until expiry/termination. Cancelled or
expired attempts continue consuming capacity while their lease could still execute.
Acknowledgement clears only its matching attempt; stale completions cannot reopen
or overwrite an artifact.

Call expire to reconcile at most sixteen expired copies and sixteen expired leases
per call. It preserves lightweight request identities and original membership
fingerprints. Expired copies are already unavailable before reconciliation. No
background cleanup loop is installed by migration or server startup.

## Rollout and rollback

Apply the additive migration before invoking either composition. The existing
shared admission and restore quota queries now count exports. Rollback first stops
export callers and reconciles all live copies and leases; deploying older admission
code while exports are still charged would stop accounting for them. Retain the
schema/identity receipts; do not rewrite published migrations or raw evidence.

Private database writers must keep deferred completion constraints through commit.
The private database role and exact codec host remain trusted. Public request,
status/download/UI flows and installed/volume qualification are outside this slice.
See [ADR 0150](../adr/0150-produce-bounded-historical-exports.md).
