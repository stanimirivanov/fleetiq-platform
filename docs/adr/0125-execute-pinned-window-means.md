# ADR 0125: Execute pinned bounded window means

## Status

Proposed, 30 October 2026.

Supersedes: none. Superseded by: none.

## Context

[ADR 0124](0124-pin-calculator-execution-evidence.md) pins replay identity and
[ADR 0007](0007-calculator-output-contract.md) pins ordered output provenance.
The first executable implementation must define numeric semantics and retain
selected facts before computing, so retry does not incorporate late arrivals.

## Decision

The trusted one-shot host selects existing semantic history, never calculated
outputs, against exact registered input/output definitions. One PostgreSQL
statement snapshot orders at most 1025 candidate references; more than 1024 is
rejected. The immutable preparation commits the original request, usable values,
quality usability and exact output definition before policy execution. Concurrent callers
resolve the committed winner with a fresh statement; changed requests fail.

The reviewed window-mean-v1 policy sums numeric values exactly and truncates the
quotient toward zero at 18 decimal places. This explicit fixed rule avoids
floating-point conversion and compile-time decimal-library defaults. Coefficients
are bounded to 1024 characters and scale to +/-256. Unsupported quality, oversized capture values or missing
samples produce insufficient evidence, not a healthy empty value; arithmetic budget
excess produces a resource-limit outcome. Neither publishes partial outputs.

A retry retains first processing time, request, preparation and deterministic
output identity even after ambiguous terminal commit. New late-data selections
require new evaluations. Preparation does not grant authorization, admit arbitrary
code, activate a release or establish a background schedule.

## Consequences

Bigdecimal becomes a direct analytics dependency for exact policy arithmetic;
it was already pinned in the workspace. No new package version, service or plugin
ABI is added. Prepared values require trusted host database-writing privileges;
terminal storage revalidates durable source/target references. Output numeric
meaning is tied to this reviewed implementation ABI and cannot change silently.

## Alternatives considered

Library-default division depends on build configuration. Binary floats lose exact
integer/decimal evidence. Re-reading inputs on retry can change results. Holding a
database transaction throughout execution instead of committing preparation does
not survive process loss. A scheduler or stream processor is unnecessary here.
