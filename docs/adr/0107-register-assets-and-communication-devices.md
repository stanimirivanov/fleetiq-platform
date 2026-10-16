# ADR 0107: Register assets and communication devices with durable attribution

## Status

Proposed, 16 October 2026.

Implementation follows approved delivery-plan PR 9; acceptance awaits human review.

Supersedes: none. Superseded by: none.

## Context

Asset and device aggregates already have tenant-safe strict persistence and
protected exact reads. Operators need commissioning commands that resolve lost
responses without silently changing identity, external identifiers or endpoints.
Existing rows have no trustworthy registration actor/time.

## Decision

Add POST to the asset collection and to an asset's device resource. Document the
existing device GET. Metadata owns the registration use case and narrow port;
PostgreSQL owns aggregate transactions and collision resolution; HTTP owns bounded
closed DTOs, listener-selected authentication and safe responses. Existing strict
inserts and read representations retain their contracts.

Consume exact current tenant metadata.manage evidence and derive actor attribution
from it. Browser writes require the existing exact-origin/CSRF guard. Reads still
require independent metadata.read. JSON is bounded at 32 KiB and each identifier
or endpoint collection at 100 entries. Tenant/actor/time cannot be chosen in JSON.

The internal tenant/resource-kind/asset key identifies create-only content.
An asset pins an exact type version and includes its name and complete typed
external identifiers. Identifier order is irrelevant; absent authority means null.
A device attaches capability to an existing same-tenant asset and includes every
endpoint ID, kind, label and declaration order. No vendor, vehicle kind, VIN,
protocol assignment or semantic attribution is assumed.

Register aggregate, set-based children and first actor/host-receipt-time evidence
in one transaction. Migration 0029 adds an immutable evidence table with composite
asset references, a conditional generated device reference and an actor reference.
Reject updates, deletes and truncation. Do not backfill legacy attribution.

Return 201 registered, 200 already_registered with the original evidence, or 409
when content differs, an external identifier is occupied, or a legacy object
lacks evidence. A different currently authorized actor may retry equal content;
it does not acquire creation attribution. Missing same-tenant references return
404. Failed inserts roll back all children and evidence.

Reuse strict aggregate SQL with an executor parameter. Canonically order the
unordered external identifiers before batching to stabilize uniqueness-lock
acquisition when concurrent requests claim multiple shared identifiers. Strict consumers still
receive uniqueness failures. Only known parent primary-key conflicts trigger
content resolution; external-identifier ownership conflicts are terminal conflicts.
Roll back the losing transaction before fresh READ COMMITTED content/evidence
queries. Never select a concurrent winner inside the conflicting insert snapshot.
Other constraints and infrastructure errors remain errors.

A commit/connection failure has an ambiguous outcome and is never clean success.
Retry the identical key/content. Registration exposes no mutation of existing
aggregates. If later maintenance changes creation content, retries return 409;
a future edit API must have its own revision/concurrency contract.

## Consequences

Commissioning gains attributed, retry-safe asset/device registration without
introducing another physical device identity or a generic persistence framework.
OpenAPI baseline.11 adds two commands and describes one existing read, preserving
all preceding operation and component shapes. Every result is no-store.

The transaction adds BEGIN/COMMIT and one evidence insert to the existing batched
aggregate statement. A duplicate attempt deliberately performs a failed strict
insert, rollback and fresh comparison. Commissioning volume does not justify
replacing clear checked SQL with an opaque upsert mechanism.

Rollback uses a follow-up build that removes the additive commands while retaining
migration 0029, evidence and registered data. A pre-0029 binary cannot simply
restart: startup validates its embedded migration inventory and rejects a newer
applied migration absent from that inventory. Do not rewind the ledger or
weaken missing-migration validation.
Legacy strict inserts remain available internally but cannot claim attributed
HTTP retry success. There is no UI editor, bulk import, endpoint replacement,
asset rename/delete, package activation, binding command or historical audit
catalogue in this slice.

## Alternatives considered

Returning success for every duplicate hides collisions. Overwriting actor/time on
retry corrupts history. Backfilling a deployment actor/time invents provenance.
Requiring a separate device ID contradicts the existing capability model.
Comparing raw JSON makes identifier order, omitted authority and serialization
details accidental domain meaning. A new command-ID framework is unnecessary
for create-only resources with stable internal identities.
