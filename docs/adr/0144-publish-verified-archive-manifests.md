# ADR 0144: Publish verified archive manifests

## Status

Proposed, 6 November 2026.

## Context

Immutable uploads alone cannot establish archive coverage. A crash can leave an
uploaded object without committed metadata, while a late interpretation can have
an earlier event time or identity than previously archived evidence.

## Decision

History owns the archive job, verified object descriptor and publication ports.
The existing archive adapter implements the store port using its bounded codec
and conditional create/read-back verification. PostgreSQL owns frozen exact
membership and the final manifest transaction. Vendor types stay outside history.
No new runtime dependency is added; the server's real-provider test reuses the
reviewed object_store dependency as a development-only edge. A package-local
cargo-machete exception records its real AwsAuthorizer/HttpRequest use in the
shared fixture source outside the server crate scan; no advisory/license waiver
or runtime dependency is introduced.

An explicit trusted server archive_once operation performs one tenant turn with
a twenty-second deadline. It starts no scheduling loop and exposes no HTTP route.
The caller establishes tenant authority and supplies operator-owned storage
configuration plus a migrated pool. Absent/disabled policy does no archive work.
Each attempt reserves one existing tenant job slot and 2 MiB for thirty seconds.
Statements are bounded at two seconds. Cancellation drops in-flight work; committed
selection and reservation recover after their database leases expire.

Discovery anti-joins complete interpretation identities against durable membership,
without an event-time or identity-sequence watermark. Under the tenant policy lock,
it selects at most sixteen currently unassigned interpretations. A conservative
32 KiB UTF-8 PostgreSQL JSON source/binding/history row cap bounds each candidate;
text/decimal/unit fields must also fit the codec’s 8 KiB field cap before selection.
Codec logical and 2 MiB physical limits still apply without truncation.
Oversized evidence remains hot and unassigned, not silently archived. Discovery
may time out at retained volume; it reports failure rather than claiming coverage.
No hot-age cutoff is a completeness claim: this slice can archive any eligible
interpretation and retains all hot rows.

Membership commits before external I/O, is tenant-qualified and unique across
jobs, and is immutable once claimed. Retries reuse the same ordered identities.
Claiming increments a generation. Completion locks tenant state and validates
current policy revision, exact reservation, generation and unexpired lease before
recording whole-object SHA-256/length and releasing the reservation atomically.
Exact completion retries return the existing outcome without another publication;
a divergent descriptor or stale generation cannot replace it. Triggers reject
membership mutation and terminal manifest update/deletion. Only committed jobs
with complete object evidence are readable as manifests.

The descriptor constructor is a validating value constructor, not a cryptographic
capability. Trusted implementations of the store port must verify the exact
selection. Database/application roles remain trusted; arbitrary privileged SQL
cannot be treated as object verification. This slice publishes exact identity
coverage, not interval completeness or permission to reclaim evidence.

## Consequences

Apply migration 0054 before using the new composition. It adds empty tables,
indexes and guards without rewriting history or activating a worker. Turning off
policy fences pending completion; in-flight reservations reconcile by expiry.
Failed verification ends its valid attempt and releases its budget. Unknown
completion outcomes remain recoverable from frozen membership and manifests.
Unreferenced uploads can remain after failure; no object deletion is authorized.

Manifest bytes are exact object accounting evidence per publication; existing
storage-usage HTTP archive totals remain explicitly not_measured. Restore,
retention eligibility/reclamation, aggregate object accounting, provider durability,
installed worker packaging and retained-volume qualification remain later work.
The existing schema checker rejects older binaries against newer migrations,
so coordinate a quiescent schema/binary cutover rather than claiming mixed-version
rolling compatibility. Operational rollback disables archive callers on a compatible
post-0054 binary and preserves the additive tables, manifests and objects; it
neither removes hot history nor deletes archive evidence. No down migration is
authorized.

## Alternatives considered

- Publish before upload/read-back: rejected; failed uploads would claim coverage.
- Event-time or generated-ID high-water mark: rejected; late or reordered commits
  could be skipped permanently.
- Replace membership on retry: rejected; crashes must converge on one selection.
- New queue or runtime dependency: unnecessary; existing budgets, SQL transactions,
  codec and immutable object adapter supply the required boundaries.

## Verification

Real PostgreSQL tests exercise absent manifests, partial evidence rejection,
immutable membership, tenant/policy fences, concurrent bounded selections,
expiry, exact duplicate completion, failed verification and earlier late arrivals.
A real PostgreSQL plus digest-pinned S3 test uploads without committing, cancels
a paused-provider retry, rejects corrupted stored bytes, then verifies that an
exact retry commits one manifest. Existing S3 qualification remains mandatory.
Full acceptance and final-commit supply-chain verification are required.
