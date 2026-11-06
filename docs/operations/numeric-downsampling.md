# Versioned numeric downsampling

## TL;DR

- Production is an explicit trusted call; migration/startup schedule nothing.
- Select an exact tenant, installed property version, aligned window and bucket width.
- Configure enough existing lifecycle allowance for a 2 MiB turn plus live staging.
- Summaries are immutable derived snapshots, with raw interpretation lineage.
- Missing coverage fails; successful summaries never imply permanent completeness.

## Scope and arithmetic

NumericRequest selects an exact target, NumericSpec and optional RestoreTicket.
NumericSpec requires complete epoch-aligned half-open buckets: start inclusive,
end exclusive, at most seven days and 64 buckets. Negative Unix times are supported.
Each complete interpretation key counts once; binding revisions and event-time
bases remain distinct, matching raw history semantics.

numeric-fixed-v1 supports integers/decimals in the definition's exact canonical
unit. No conversion, interpolation, correction collapse or calculator substitution
occurs. Good samples contribute to count/min/max/sum. Uncertain/invalid samples
remain in lineage and total/excluded counts. A zero-count bucket has null extrema,
zero sum and an undefined mean. For a nonempty bucket, sum/count is the exact
arithmetic mean; choosing a presentation rounding rule belongs to its consumer.

The coefficient-text/scale envelope matches existing calculator limits (1024
characters, scale -256 through 256). Persisted numbers are normalized plain decimal
strings. Input is capped at 100 unique interpretations with one look-ahead; larger
windows/data fail InputLimit rather than silently truncating. Choose a narrower
window and explicitly retain the changed scope.

## Sources and coverage

Hot selection and ready staging are captured together in one repeatable-read view.
The sealed snapshot only passes its selected file to the existing exact decoder.
Membership, tenant, target/window and overlapping original evidence are verified
before reduction. A ready file that later expires remains valid captured evidence;
the result describes its capture time, not present restore availability.

hot_snapshot describes available interpreted hot rows. hot_and_selected_manifest_snapshot
also includes one explicitly chosen verified restore generation. Neither describes
all raw accepted messages, unfinished semantic interpretation or every archive.
Unavailable/pending/cancelled restore and known reclaimed target evidence outside
the selection produce CoverageUnavailable. Missing data cannot appear as a
complete empty interval.

## Explicit production and retries

After migration 0057, use the private trusted server composition
produce_numeric_summary_once with a migrated pool and validated NumericRequest.
The caller establishes tenant authority. This PR adds no CLI/HTTP command or loop;
M06-PR12 owns the summary read API.

An existing configured tenant policy must admit one slot and 2 MiB in addition to
live restore storage. The whole turn has twenty seconds; database statements and
lock waits have two seconds. Disabled policy/inactive tenant, denied admission,
unsupported context and unavailable coverage remain distinguishable failures.
Cancellation keeps uncertain capacity until its durable expiry.

Unchanged full content returns the original series/revision receipt. A late input
creates a new immutable revision. An older, never-published capture that would
remove already-published inputs is rejected as StaleSnapshot; capture again.
Concurrent identical producers converge without duplicate revisions. Observation
time and revision order are not source arrival/completeness watermarks.

## Audit and rollback

numeric_summary_series owns exact target/window/resolution/algorithm/restore scope.
numeric_summaries stores immutable buckets, canonical input keys/values/quality,
coverage, capture time, admission ID and content fingerprint. Full JSON content is
compared on retry. SQL validates geometry/counts/format and rechecks complete live
tenant admission at commit; the trusted host owns exact arithmetic/source validation.

No raw source, archive, hot payload or calculator evidence is changed. Rollback
stops callers and leaves additive schema/derived records intact. UI, automatic
deployment, physical cost measurement and production capacity qualification remain
later work. See [ADR 0149](../adr/0149-produce-versioned-numeric-downsampling.md).

Private SQL writers must retain the initially deferred completion constraint
through commit; forcing it early and then changing admission is outside the
supported publication protocol. The producer owns its transaction and never
forces this constraint early. Database owners and the decoder host remain trusted.
