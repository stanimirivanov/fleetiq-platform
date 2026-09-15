# ADR 0037: Verify ingestion recovery at durability boundaries

## Status

Accepted, 15 September 2026.

## Context

FleetIQ separately tests host validation, accepted-message storage,
decoded-result storage, and their ownership transitions. Those tests do not
prove that public components preserve identities and gating when composed.

An append error is ambiguous: storage may have rejected the operation, or its
commit may have succeeded before the response was lost. Raw acceptance must not
release deferred effects until a retry confirms durability. A decoded-result
failure must return the complete batch and replies already authorized by raw
durability. Empty results need the same recovery evidence without child rows.

## Decision

Maintain a PostgreSQL integration harness that begins with a synthetic processed
frame and crosses the public host, application, and persistence boundaries.
Seed only device-admission metadata directly; production adapters write every
ingestion fact.

Use a test-only fault decorator that either fails before append or calls the real
adapter to completion, discards its successful response, and returns a
connection-reset error. Recover the owned pending value and retry it unchanged
through the real adapter. Assert append dispositions and row counts at each
stage. Cover non-empty and empty results plus repeated whole-frame delivery.

Verify immediate processing replies, withholding of raw-durability replies on
an unconfirmed raw append, and continued reply eligibility during decoded-result
retry.

## Consequences

- Composition regressions in identity propagation, ownership, and reply gating
  fail in a focused integration suite.
- Response-loss recovery is deterministic rather than timing-sensitive.
- Tests prove exact retry dispositions, stable observation identities, and no
  duplicate durable rows.
- PostgreSQL tests retain the usual migrated SQLx database prerequisite.
- A suppressed response after completed commit does not reproduce every driver
  or distributed-system failure.

## Alternatives considered

### Rely only on component-level tests

This leaves errors spanning individually correct components undetected.

### Kill PostgreSQL during each integration test

The interruption point is timing-sensitive and cannot reliably distinguish
failure before commit from response loss after commit. That belongs in
deployment-shaped failure testing.

### Add fault controls to production adapters

A test-only decorator uses the existing ports without expanding runtime APIs.

## Out of scope

- interruption while a commit is in progress;
- process, host, or storage termination;
- PostgreSQL replication guarantees;
- broker acknowledgement, redelivery, and consumer checkpoints;
- automatic retry timing, limits, backoff, and dead-letter handling;
- transport reply writes and delivery deduplication; and
- production ingestion-worker lifecycle and connection ownership.
