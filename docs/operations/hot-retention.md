# Verified hot-payload reclamation

## TL;DR

- Migration and policy configuration reclaim nothing. Approval is absent by default.
- Review ADR 0148 and qualify immutable retained storage before opting in.
- Explicitly approve one policy revision, then call one trusted bounded manifest turn.
- Preview tokens and operations.read never authorize execution.
- Revoke before rollback; code rollback cannot recreate removed hot fields.

## Supported payload and evidence

Only three property-history projection timestamps are cleared, together.
Interpretation identities, source wire/decoded values, exact binding revisions,
deduplication and foreign-key anchors remain. The immutable archive retains original
timestamps, values and first-resolution provenance. Current projection pointers and
pending projection work and terminal calculator references are protected. A manifest containing a current winner is
retained in full; selection never silently clears a subset.

Removed logical field bytes are 24 per reclaimed interpretation. These tiny fields
are the initially supported payload class. Audit/anchor overhead can exceed the
removed bytes; neither the preview nor this executor claims net or physical savings.
Measure vacuum, indexes, storage and retained-volume effects separately.

## Deployment and explicit approval

Apply migration 0056 with semantic writers quiesced and an explicit lock_timeout.
The new CHECK is NOT VALID: existing rows already satisfy the prior NOT NULL rules;
future writes are checked immediately without a full history scan under the DDL
lock. Exclusive lock acquisition can still wait. Separately schedule validation
if catalog/planner proof is needed. Deploy compatible binaries everywhere before approving retention; old binaries
assume non-null payload. Migrate and deploy alone leave behavior unchanged.

The private trusted operator interface is PostgresHotRetention::approve with tenant,
exact current policy revision and active actor. Human review must cover durable
immutable object retention, restore qualification, hot duration and operational
impact. This is not an HTTP flow and no workload/browser grant is elevated.

The trusted server composition reclaim_once takes a RetentionTarget containing
tenant, explicit published manifest and expected policy revision, plus existing
operator-owned S3 configuration. It starts no automatic loop. Keep original target
parameters for exact retry reconciliation. Use PostgresHotRetention::revoke to
prevent subsequent mutation; policy changes require a new explicit approval.

## Safety, concurrency and interruption

One turn reserves 2 MiB through the existing tenant budget, verifies complete file
digest/length/profile/tenant, and compares decoded original evidence with all hot
members. No unverified upload or caller-controlled key becomes authority.

A bounded transaction rechecks active tenant/actor, approval and current policy,
conservative verified receipt age and known replay, semantic, calculator, restore
and projection dependencies. Coarse table locks exclude new dependency writers
during these checks. They can briefly block unrelated tenants; two-second
statement/lock limits fail closed instead of extending contention indefinitely.
Schedule low-rate turns and measure impact before enabling at volume.

The receipt and complete all-field transition commit together. A cancelled or failed
transaction leaves no partial receipt/payload state. A successful exact retry reports
AlreadyReclaimed without fetching the provider again. An uncertain result must be
retried with its original manifest/policy target. Provider/codec/database failures
remain redacted errors. Cancellation may retain capacity until thirty-second expiry;
the twenty-second turn deadline never silently releases uncertain work early.

## Reads and recovery

Hot-only history returns archived-history 409 when that target has any reclaimed
interpretations, even if a particular requested interval might otherwise be hot.
Exact hot provenance also returns 409. This avoids a complete-looking empty result.

Request and verify the explicit manifest restore, then select its generation with
hot_and_restored history. Original evidence/provenance is returned with explicit
selected-manifest coverage. No automatic scan or replay is performed. Restoration
does not refill immutable hot fields. Future calculator preparation for incomplete
targets fails closed; terminal prepared/evaluated snapshots remain retained.

Before code rollback, revoke approval, stop callers and retain compatible history/
restore readers. Restore archived evidence through bounded staging. Do not disable
guards, rewrite anchors or claim that an old binary is compatible with reclaimed
payloads. Packaging, provider durability and production capacity remain separate
qualification work.

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
