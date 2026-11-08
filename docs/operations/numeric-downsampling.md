# Versioned numeric downsampling

## TL;DR

- Production uses an explicit trusted command. Migration and startup schedule nothing.
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

Run `fleetiq-server summarize-once` from a trusted operator job after applying
the current migrations. The caller establishes tenant authority. The binary
validates the selection before connecting and checks the installed schema in
read-only mode. Set `DATABASE_URL` through private configuration and set the
following target inputs.

| Environment input | Value |
| --- | --- |
| `FLEETIQ_SUMMARY_TENANT_ID` | Exact tenant |
| `FLEETIQ_SUMMARY_ASSET_ID` | Exact asset |
| `FLEETIQ_SUMMARY_PROPERTY_ID` | Exact property identifier |
| `FLEETIQ_SUMMARY_PROPERTY_VERSION` | Positive property definition version |
| `FLEETIQ_SUMMARY_START_MS` | Inclusive signed Unix millisecond boundary |
| `FLEETIQ_SUMMARY_END_MS` | Exclusive signed Unix millisecond boundary |
| `FLEETIQ_SUMMARY_RESOLUTION_MS` | Positive bucket width in milliseconds |
| `FLEETIQ_SUMMARY_MANIFEST_ID`, `FLEETIQ_SUMMARY_RESTORE_GENERATION` | Both unset for hot history, or both positive for one exact restore |

Exit zero prints `summary series=<id> revision=<number>`. Exit one prints a
redacted error, such as `invalid numeric summary target`,
`numeric summary rejected: BudgetDenied` or
`numeric summary rejected: CoverageUnavailable`. Keep the original inputs
for an exact retry. The command has no inline arguments or recurring mode.
See [ADR 0155](../adr/0155-expose-one-shot-numeric-summary-command.md).

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

## Bounded summary reads

GET /api/v1/tenants/{tenant_id}/assets/{asset_id}/properties/{property_id}/versions/{version}/summaries
requires current telemetry.read with the existing workload or browser authentication.
Supply canonical signed start_at_ms/end_at_ms strings, positive resolution_ms,
and optional limit (1–64, default 64). Bounds match the producer: seven days,
64 full epoch-aligned buckets. The exact window/resolution must already be published;
missing publication returns 404. Reads never trigger production or raw fallback.

The response explicitly identifies representation=numeric_summary, algorithm,
resolution, immutable series/revision and original observed_at_ms. Numeric values,
IDs, revisions and times are exact strings. Buckets include total/count/excluded,
min/max/sum. Mean is sum/count; count zero has no mean. Coverage describes that
captured evidence and never promises interval completeness or current staging.
Supply manifest_id and restore_generation together to select a published restored
scope; live staging is unnecessary once the snapshot has been published.

Use next_after as after with the same tenant, target/version, range, resolution
and restore selection. Continuation pins the original revision even if late inputs
produce a new head. Restart without after for the latest publication. Tokens are
bounded opaque positions, not credentials; each page reauthenticates/authorizes.
Malformed, cross-scope or nonexistent publication positions return 400. The route
inherits configured history work admission (429 on denial), no-store responses,
and the SQL two-second timeout; disabled policy preserves existing read behavior.

HTTP baseline.34 is additive. Regenerate clients before consuming summaries.
Rollback removes this route/callers and leaves all published evidence intact.
