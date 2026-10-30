# ADR 0127: Explain exact window median/MAD anomalies

## Status

Proposed, 30 October 2026.

Supersedes: none. Superseded by: none.

## Context

M05 needs one explainable detector over the bounded semantic input preparation
from [ADR 0125](0125-execute-pinned-window-means.md), usable by the existing
one-shot command and [scheduled worker](0126-schedule-fenced-calculator-work.md).
Mean arithmetic and output units cannot silently acquire different meanings.

## Decision

Admit `window-mad-v1` as a distinct immutable first-party ABI. Use the newest
sample in canonical event-time/evidence-ID order as the observation; all preceding
samples form the baseline. Require at least three baseline values and a total
count of max(configured minimum, 4), capped at 1024. Any unusable sample yields
insufficient evidence with no output, never a healthy flag.

Compute the baseline median, median absolute deviation (raw MAD), observation's
absolute deviation and threshold using exact decimal/integer arithmetic. Even
medians use exact midpoints. Flag strictly greater than six times MAD; equality
is normal. Zero MAD flags any nonzero departure from the median, and an unchanged
flat series is normal. Six raw MAD is a fixed operational rule, not a calibrated
normal-distribution z-score. Coefficients and scales retain the mean's 1024-character
and +/-256 budgets; excess yields a resource-limit terminal result.

Return typed explanation values from policy; persist a dimensionless decimal
flag, 0 or 1, with the existing exact manifest, immutable prepared values and
versioned release. These retained values reproduce the explanation without
reselection, a wall clock or a separately mutable summary. This slice adds no
explanation HTTP API. Input units remain pinned by exact semantic property;
registration requires dimensionless decimal output for MAD, while mean continues
to require equal canonical input/output units. Migration 0042 enforces both paths.

## Consequences

No dependencies, learned state or external model runtime are added. Threshold,
minimum baseline and zero-MAD behavior cannot change under this ABI; different
semantics need another reviewed implementation version. Strong trends, irregular
sampling and flat-baseline noise can trigger this deliberately simple rule.
It is an operational signal, not a statistical confidence or learned prediction.

New preparation pins the implementation; older preparations lacking that field
are known mean-only selections and remain replayable. Upgrade checked-schema
workers before admitting MAD releases. Rollback stops affected workers and retains
immutable evidence; an older binary must not interpret MAD preparations as means.

## Alternatives considered

Augurs was evaluated in the approved plan; adapting the median/MAD pattern with
existing BigDecimal avoids a new dependency for a small exact deterministic rule.
Floating-point modified z-scores risk lossy large-integer conversion and undefined
zero-MAD division. Including the observation in its own baseline can mask spikes.
Configurable or learned thresholds add configuration semantics beyond this slice.
