# ADR 0136: Recover work with monotonic fences and immutable receipts

## Status

Proposed, 31 October 2026.

Supersedes: none. Superseded by: none.

## Context

[ADR 0115](0115-persist-fenced-stateful-protocol-replay.md) and
[ADR 0126](0126-schedule-fenced-calculator-work.md) bounded initial work to three
claim generations. Rewinding those generations would allow stale effects to
commit. M05 needs explicit operator recovery while preserving frozen inputs,
original evaluation identity, immutable source evidence and remote ambiguity.

## Decision

Add independent operations.manage; it implies no read, metadata, analytics,
protocol or alert authority. Baseline.28 exposes it through existing delegation
and discovery, with no automatic grant. Regenerate strict clients before delegation.
Write commands require current management authority plus the listener mutation
guard. Bounded target preparation requires both operations.read and operations.manage
from the same current actor/tenant. Ordinary diagnostics still omit identities and
digests; the stronger command-preparation boundary exposes only exact safe keys.

Use tenant/request-ID immutable receipts binding original actor, family, operation,
target, expected/result generation and database time. Recheck current authority
in the database, even for repeats. Exact retries return the original receipt
without reapplying effects after later state changes; changed actor/body conflicts.
Serialize request identity, then control or work ownership, with bounded statements.
Replay recovery takes session-before-work locks, matching completion. Calculator
completion shares the tenant lock, compatible with command authority checks.

Separate replay/calculator per-cycle attempts from monotonic generations. Backfill
attempts from old generations, preserve three attempts per cycle, and advance the
fence on recovery before rearming. Notification retains five attempts and advances
its existing fence. Guard finite counter capacity; never wrap or rewind. Retry
only one failed, currently eligible position without a live lease. Completed work
is never reopened. Preserve replay pre-state/execution binding and calculator
request/prepared inputs; intentional reinterpretation requires a new evaluation.
Notification recovery changes no target, acceptance history, incident identity or
resolved reconciliation horizon and cannot recall remote effects.

This adds an explicit audited exception for terminal work metadata, not accepted
inputs, results, checkpoints, lifecycle transitions or intents. Replay/calculator
triggers require matching immutable recovery proof for terminal rearm or attempt
reset. Receipts and queue-control identities cannot be edited/deleted.

Absent controls mean generation zero and unpaused. Pause/resume compare the queue
control generation independently of any work generation. Compatible seeding/claim
statements that observe pause do no new work; in-flight statements/completion may
finish. Ingress remains durable; alert pause is not rule deactivation/resolution,
notification pause is not suppression, and pause is never proof of drain. Retry
while paused does not resume. Alert evaluation has no failed lease to reset.

## Consequences

Migration 0051 requires quiescing old workers/servers. It backfills mutable retry
metadata, flushes deferred checks before further ALTERs, adds candidate indexes,
and extends action checks last. Review scans, table locks and runtime grants.
All original work/fact content is retained. Old generation-as-attempt binaries must
not restart after migration; rollback needs a compatible forward build. Do not
weaken triggers, rewrite ledger checksums, prune work or grant elevated fallbacks.

The UI is PR19 and installed qualification PR20. Prepare at most 32 targets plus
a coverage sentinel under a two-second statement deadline; commands use three
seconds and bounded HTTP turns. Candidate lists are snapshots, not health, a drain
barrier or complete monitoring. Ambiguous remote delivery stays at least once.

## Alternatives considered

Resetting generations breaks fencing. Replacing inputs, deleting terminal work or
force-completing it invents evidence. Reusing business management silently widens
permission. Automatic resumption or bulk retries hide operator intent. Adapt the
existing immutable-command, database-clock lease and optimistic-selection patterns
instead; add no dependency, broker or general SQL console.
