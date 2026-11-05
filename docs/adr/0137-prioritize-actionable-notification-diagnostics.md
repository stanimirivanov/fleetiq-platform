# ADR 0137: Prioritize actionable notification diagnostics

## Status

Proposed, 5 November 2026.

Supersedes the notification sampling policy in
[ADR 0135](0135-read-bounded-operational-work-evidence.md).

## Context

A bounded identity prefix of retained notification positions can consist entirely
of completed reconciliation history. It can hide an exhausted or due delivery
even when that delivery is the tenant's only actionable work.

## Decision

Classify tenant notification positions in the existing repeatable-read read-only
snapshot before selecting the sample. Exclude completed reconciliation positions.
Select failures first, then eligible deliveries, then leased, blocked and deferred
positions together. Identity ordering breaks ties within each priority. Preserve
that ordering when consuming the sentinel so a lower priority position cannot
displace a selected failure.

Retain active lease precedence, newer terminal successor rearming, pause/backoff
rules, the 64-position sample, one truncation sentinel and closed failure
categories. The HTTP response shape and authority remain compatible.

## Consequences

Notification sample counts and truncation now concern unfinished positions.
Retained completion history contributes neither counts nor truncation.

Priority selection examines tenant-local retained positions, so its cost grows
with history. The existing two-second statement deadline and five-second HTTP
turn remain enforced. Timeouts return a redacted failure. Qualify retained volume
before production rollout. Indexed maintained summaries require separate scale
evidence and are outside this correction.

Reverting the query and its metadata restores the previous sampling policy.
No migration, source-state mutation, new permission or dependency is required.

## Alternatives considered

Keeping the identity prefix preserves seek cost but allows completed history to
hide all actionable work. Reserving several identity-prefix slots per state still
requires classifying notification states before sampling. A maintained summary
could reduce read cost, but adds state transitions and schema ownership beyond
this query correction.
