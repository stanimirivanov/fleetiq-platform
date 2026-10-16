# Relationship and signal-binding revision commands

## TL;DR

Use exact tenant metadata.manage to POST complete facts to an explicit revision
path. Start at revision 1; a correction uses the exact successor and preserves
earlier facts. Retry the same identity, revision and content after an ambiguous
response. Keep physical effective time separate from server-recorded time.
Reads independently require metadata.read.

## Prepare identity and references

Select a currently active tenant and provision its exact grants through
[tenant-access administration](tenant-access-administration.md). Sign in through
[browser identity](browser-identity.md), or use the configured workload credential
only from a trusted server-side operator tool. One listener selects one mode.
Browser writes include one exact public Origin and `X-FleetIQ-CSRF: 1`.

Publish exact [semantic definitions](semantic-definitions.md) and
[register assets and device endpoints](metadata-registration.md) first.
Relationships need same-tenant source/target assets and an exact relationship
type. Bindings need a registered same-tenant device endpoint, target asset, and
exact property supported by that asset's pinned type. A decoded signal ID is an
extensible name, not proof that a deployed decoder currently emits it.

## Record a relationship

POST `/api/v1/tenants/tenant-a/relationships/edge-a/revisions/1`:

```json
{
  "relationship_type": { "id": "physical.contains", "version": 1 },
  "source_asset_id": "assembly-a",
  "target_asset_id": "component-a",
  "effective_interval": { "start_ms": 0, "end_ms": null }
}
```

The source and target roles are directed. The interval is half-open: start is
included, finite end excluded. Omit end or use null for an unbounded interval.
Finite end must exceed start. All instants use signed Unix milliseconds.

## Record a signal binding

POST `/api/v1/tenants/tenant-a/signal-bindings/binding-a/revisions/1`:

```json
{
  "source": {
    "device_asset_id": "gateway-a",
    "endpoint_id": "can-primary",
    "signal_id": "bus.voltage"
  },
  "target": {
    "asset_id": "component-a",
    "property": { "id": "electrical.voltage", "version": 1 }
  },
  "effective_interval": { "start_ms": 0, "end_ms": null }
}
```

Source topology and semantic target remain distinct. A device may observe itself
or another asset. Property existence alone is insufficient: its exact version
must be a member of the target asset's pinned type. Wrong tenant, missing
endpoint, missing type/property or unsupported target property cannot append.

## Correct and inspect history

Send the complete corrected fact to revision 2, then 3, and so on. Do not PATCH,
delete or rewrite revision 1. A correction may change endpoints, source, target,
type or effective interval. The existing stable identity keeps their lineage.

A correction replaces the complete known fact for that stable identity; its
effective interval is not a patch window that merges earlier revisions. To record
a real physical handover, close the old identity at the boundary and create a
different identity starting there. These are separate atomic appends, not an
atomic multi-fact handover; concurrent readers can observe intermediate gaps or
overlaps. Review the resulting snapshots explicitly.

GET either exact command path to retrieve its original read representation.
Revision is positive-u64 decimal text, not a JavaScript number. Leading zeros
in paths are accepted and normalized in responses; overflow, zero, signs and
nondecimal syntax are rejected. Exact missing revision returns 404, never latest.

Existing [relationship snapshots](asset-relationship-snapshot.md) and
[binding snapshots](signal-binding-snapshot.md) select the latest revision known
at an inclusive recorded-time cutoff before applying its half-open effective
interval. A correction therefore cannot rewrite what was known earlier.

## Outcomes and recovery

| Status | Meaning | Operator action |
|:--|:--|:--|
| 201 | Fact, head and actor record committed together | Retain ID, revision and receipt. |
| 200 | Identical attributed exact revision already exists | Retain original actor/time; no new fact was appended. |
| 400 | Invalid shape/value, nonpositive/overflow revision, duplicate field or body over 32 KiB | Correct the request; do not add tenant, actor or recorded-time fields. |
| 401/403 | Missing identity or current authority; browser mutation guard failure | Restore identity, exact tenant grant or configured origin/header. |
| 404 | Required same-tenant reference absent/incompatible | Commission correct exact references first. |
| 409 | Different content, sequence gap/exhaustion, clock regression or unattributed legacy revision | Inspect exact history and reconcile; never blindly increment revision to defeat a collision. |
| 500/network loss | Outcome may be ambiguous | Retry the same identity/revision/content after recovery. |

Successful receipts contain outcome, stable id, normalized revision, recorded_by
and recorded_at_ms. Another authorized actor retrying equal content receives the
first actor/time. A retry of revision 1 still succeeds after revision 2 exists.
Legacy strict writes have no historical attribution: identical legacy retries
return 409; the next explicit correction can have new attribution.

All responses are no-store and hide database details. These commands do not
trigger automatic semantic reprocessing or enforce graph-wide cycles,
cardinality or overlap policy. Recorded time is host-owned and cannot decrease:
investigate clock health if an otherwise valid successor conflicts.

## Rollout and rollback

Startup applies additive migration 0030 before listening. It adds empty immutable
actor-record tables and tenant-safe exact-fact/actor references, with no historical
backfill or existing-row rewrite. Foreign-key installation briefly locks related
tables; schedule deployment accordingly.

Rollback through a follow-up build that removes commands while retaining
migration 0030, all facts and evidence. A pre-0030 binary cannot restart against
the newer migration inventory. Do not rewind checksums/ledger or disable
missing-migration checks. Protect database privileges as for existing audit tables.
