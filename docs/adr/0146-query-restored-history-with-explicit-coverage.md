# ADR 0146: Query restored history with explicit coverage

## Status

Proposed, 6 November 2026.

## Context

A hot-only page cannot describe restored archive availability. Transparent object
scans would undermine bounded history reads, and missing coverage must not look
like a complete empty historical interval.

## Decision

Baseline.31 extends the existing property-history list with opt-in mode
hot_and_restored and required manifest_id/restore_generation. Absent mode or hot
keeps legacy payloads, tokens and semantics. Restore parameters are rejected in
hot mode. Identifiers are canonical positive signed-64-bit strings; reads never
create restores or choose a latest generation implicitly.

Every request authenticates and authorizes telemetry.read before feature errors
or telemetry access. PostgreSQL observes the exact tenant/manifest/request state
and optional verified file in one statement. No object-store calls are made.
The existing bounded codec reconstructs ready staging independently of hot values.
Invalid, missing, replaced, cancelled or expired staging is unavailable; pending
is restoring; a published manifest with no request is archived. Ready/valid is
restored. Database failures remain errors, never empty success.

The response adds coverage only in opt-in mode: hot=queried, the archive state,
requested manifest/generation and scope=selected_manifest_only. This certifies
neither interval completeness nor all manifests. Empty results remain qualified
by that scope and availability; hot querying does not infer ingestion health.

A history-owned merge port combines at most sixteen restored interpretations
with the normal hot page, deduplicating the complete immutable identity. Conflicting
content for the same identity fails closed. Rows filter to the exact property
version/window. Set-wise PostgreSQL key ordering preserves hot text collation,
Unicode identifiers, numeric binding revisions and time-basis ties rather than
substituting Rust string ordering. With at most 100 hot candidates and sixteen
restored candidates, ordering processes at most 116 keys. The hot look-ahead is
retained when producing the merged continuation; no query-per-row lookup is added.

Opt-in tokens bind tenant, asset, exact property version, window, mode, manifest,
request generation and observed coverage state. State changes require restarting;
old cursors return 400. Tokens grant no authority and remain bounded at 8192 bytes.
Unavailable selections issue no continuation and reject supplied continuations;
callers can select a valid restore generation or use hot mode for hot-only paging.
Pages still are not a snapshot: late hot appends behind a cursor require restart.

Existing seven-day and 100-result limits remain. Opt-in work always has a five-
second whole-operation limit and 1 MiB response cap, including disabled-policy
reads; configured tenant admission still applies. Shared staging/work accounting
requires headroom for live copies plus the 1 MiB history reservation. Exhaustion
or oversized output returns existing explicit 429 responses, not empty history.
Legacy disabled-policy hot behavior remains unchanged.

## Consequences

No migration, dependency, worker, storage input or HTTP major-version change is
added. Regenerate clients before opting in to baseline.31. Old consumers that do
not opt in retain the exact two-field hot-history page. Exact interpretation
lookup remains the existing hot-only operation; opt-in list rows already carry
original values and source/attribution provenance.

Rollback stops callers opting in and uses legacy hot mode; restore staging,
archive evidence and hot rows are unaffected. This slice adds no restore commands,
transparent cross-tier scanning, reclamation, UI or production capacity claim.
Retained-volume, deployment and provider durability qualification remain later work.

## Alternatives considered

- Empty fallback for absent/corrupt coverage: rejected; it would hide unavailable history.
- Automatic archive scans or latest-generation fallback: rejected; selection and work must be explicit.
- Rust sorting of mixed keys: rejected; database text collation determines existing pagination.
- Reuse hot cursors across restore transitions: rejected; newly available rows behind a cursor could be skipped.

## Verification

HTTP/real PostgreSQL tests cover archive/restore states, exact large integers,
overlap deduplication, state/generation/expiry/cross-mode cursor rejection and
fresh authorization. Separate PostgreSQL tests include restored-only rows and
exclusive continuation. A faulty-writer fixture proves invalid ready bytes become
unavailable coverage with no cursor. Actual responses validate against the updated
bundle. OpenAPI checks, full acceptance and final-commit supply-chain checks apply.
