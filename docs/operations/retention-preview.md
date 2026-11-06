# Retention eligibility preview

## TL;DR

- GET /api/v1/tenants/{tenant_id}/retention-preview requires current operations.read.
- The report is a read-only sample of at most 100 interpretations, not deletion authority.
- Identity/lineage anchors are always retained. Explicit trusted reclamation is separately approved; this route authorizes nothing.
- Context lasts sixty seconds; a recheck always reads fresh state.

## Scope and protection

The sample uses stable complete-identity order and reports has_more when additional
interpretations exist. Candidate IDs are opaque identity digests; telemetry values
and original source keys are absent. Counts and estimates apply only to this sample.

Configured hot retention is assessed conservatively from a verified manifest lease
end after source visibility. It waits at least hot_days after that receipt, even
when event time is much older. Unverified publications and insufficient age remain
protected. No live provider integrity or durability certification is implied.

Consumer holds cover replay, semantic, calculator and restore dependencies. They
are tenant-wide and conservative, including incomplete/unknown semantic discovery, unresolved/poisoned/exhausted work,
active or legacy-unknown schedules, unfinished preparations and pending/live
restores. They indicate potential references, not process liveness. A hold may
protect a candidate even when a narrower analysis could eventually prove it safe.

## Estimates and context

The method three_bigint_projection_fields_v1 counts 24 potential logical bytes
per eligible interpretation: three projection timestamp fields. Anchors remain,
and physical disk/index/TOAST usage and net storage savings are not measured.
Source wire and decoded values are outside this reclamation scope. Existing
identity/anchor rows cannot be deleted through this endpoint.

Pass the returned preview_token in the optional preview query parameter to recheck.
The server authenticates and authorizes again and recomputes a consistent snapshot.
Expired context or changed policy/sample/protection returns 409; malformed or
cross-tenant context returns 400. Fetch a fresh preview after either response.
The public token is context, not a credential/signature or execution permit.

## Rollout and qualification

Baseline.33 adds archived/current-projection protections and a trusted reclamation
capability indicator; regenerate clients before consuming it. This route adds no
worker, scheduling or destructive behavior. Snapshot/statement failure
returns an error rather than an empty successful report.

The separately approved executor reverifies archive bytes, preserves anchors and
atomically rechecks current references and policy. Nothing in this report authorizes that
action. See [ADR 0147](../adr/0147-preview-retention-eligibility.md) for the design
and the retained-volume/provider/installed qualification boundaries. See also
[verified hot reclamation](hot-retention.md).
