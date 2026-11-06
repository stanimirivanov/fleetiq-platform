# ADR 0147: Preview retention eligibility

## Status

Proposed, 6 November 2026.

## Context

Retention must distinguish potential payload candidates from immutable identity,
deduplication and lineage anchors. Event/device times do not establish append age,
and a preview cannot authorize a later destructive action.

## Decision

Add an operations.read-protected read-only preview/recheck with at most 100
interpretations plus one look-ahead, in stable complete-identity order. A
repeatable-read snapshot combines policy revision, committed archive receipts
and known consumer holds. Statements have two-second limits within a five-second
operation. Failures remain errors, never empty eligibility reports.

Reuse the verified manifest's immutable database lease end as a conservative
age floor after source visibility. Eligibility waits at least hot_days after
that receipt, deliberately over-retaining rather than trusting old event times.
A valid receipt requires published object evidence, a positive generation and
a lease end; absent or malformed receipts remain unverified. No schema/age-column
migration or source rewrite is necessary. Provider bytes are not re-read by a
preview and must be freshly verified by any future executor.

Disabled policy, insufficient age, unverified publication or any known consumer
hold blocks payload eligibility. Holds cover unfinished/poisoned replay,
incomplete/unknown semantic discovery and unfinished/unresolved/exhausted semantics, active or legacy-unknown calculator
schedules, unfinished calculator work/preparation, and pending/live/in-flight
restore work. They are deliberately tenant-wide where narrow reachability has
not been proven; they describe potential dependency protection, not worker liveness.

Identity and lineage anchors remain retained in every case. Estimate only the
potential logical bytes of event_at_ms, resolution_effective_at_ms and
resolution_known_at_ms: three BIGINT fields, 24 bytes per eligible interpretation.
This is not whole-row deletion, physical/TOAST/index savings, net storage savings,
wire/decoded value destruction or billing. Totals describe only the sample;
has_more explicitly disclaims a whole-tenant inventory. Current immutable row
constraints remain intact and reclamation_supported is always false.

Public opaque context binds tenant, policy revision, sampled assessment fingerprint
and a sixty-second lifetime. A recheck always recomputes a fresh consistent snapshot.
Policy, membership or visible protection changes and expiry return 409; malformed
or cross-tenant context returns 400 after fresh authorization. Context is not a
signature, credential, stored permit or future-state guarantee. Clients can always
request a fresh preview. No deletion API consumes the token.

## Consequences

Baseline.32 adds the preview endpoint without changing existing routes, grants,
dependencies or persisted schema. Operations authority is independent of telemetry
authority, and no telemetry values/source keys are returned. Responses are no-store.

Conservative age and tenant-wide holds can retain more data than a narrower
future selector. A bounded identity sample can omit eligible rows elsewhere.
Both limits are explicit rather than claims of exhaustive eligibility. M06-PR10
must separately establish a supported narrow payload mechanism, preserve anchors,
reverify objects and atomically recheck current policy/consumer protections.
The destructive checkpoint still requires human review; this PR removes no data.

Rollback stops using the additive endpoint. Evidence, policy and work remain
unchanged. Retained-volume, provider durability and installed qualification remain
later work.

## Alternatives considered

- Event-time expiry: rejected because late arrivals can have arbitrarily old event time.
- A new append-time migration: unnecessary for this conservative verified-receipt floor.
- Treating the preview token as authority: rejected because dependencies can change immediately after a snapshot.
- Whole-row or physical-byte estimates: rejected because protected anchors remain and no reclamation mechanism exists.
- Optimistic narrow dependency assumptions: rejected until supported by explicit reachability evidence.

## Verification

Pure tests exercise age extremes, disabled policy, unverified archive and each
consumer family. Real PostgreSQL tests cover replay completion and registered/
active scheduling holds. HTTP/database tests cover verified receipt age, restore
holds, tenant/sample bounds, grant changes, policy/dependency rechecks and expiry.
Actual responses validate against baseline.32. Full acceptance and final-commit
supply-chain verification remain required.
