# ADR 0094: Page immutable interpretations for one decoded observation

## Status

Accepted, 3 October 2026.

Supersedes: none. Superseded by: none.

## Context

ADR 0093 exposes one exact durable semantic interpretation. Operators and
future projections also need to inspect all recorded attributions of one
decoded output, including a corrected binding revision or a different
event-time basis. Invoking the point read for each interpretation would add a
query per result. A broad asset history feed needs separate filtering,
authorization, index, and currency policies.

## Decision

`fleetiq-history` adds a bounded source-scoped page to
`AssetPropertyChangeView`. A request specifies tenant, exact decoded-output
identity, a limit from 1 through 100, and an optional exclusive cursor for
that same output. The order is binding ID, binding revision, then event-time
basis, matching the suffix of the immutable history primary key. The result
contains complete `StoredPropertyChange` values and a next cursor only when
another row exists.

PostgreSQL reads at most limit plus one joined history and exact binding
revision rows, then loads the common decoded observation once for the page.
It validates each reconstructed interpretation against that source. Empty
history does not require a source read. A missing source for a referenced
history row is corruption. The query never falls back to another tenant,
decoder release, output, or binding revision. No schema change is needed
because the exact source predicate and ordering use the history primary-key
prefix and suffix.

Pages are keyset traversals of an append-only set, not a repeatable snapshot.
An interpretation appended ahead of a cursor can appear on a later page; one
appended behind the cursor requires starting a new traversal. The cursor does
not encode a knowledge-time cutoff or imply a winning current value.

## Consequences

Callers can inspect historical corrections and time-basis choices with a
bounded number of rows and two database reads per nonempty page. The read
contract remains transport independent. An authorized HTTP endpoint,
asset-wide history query, current-value selection, and live projection
policy remain separate work.

## Alternatives considered

Calling the exact read in a loop causes N plus one history queries and makes
latency depend on page size. Offset pagination can skip or repeat rows when
append traffic changes the result set. A materialized source-to-asset history
index is premature for this exact-source lookup.