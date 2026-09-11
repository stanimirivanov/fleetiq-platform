# ADR 0006: Asset-property change contract

- Status: accepted
- Date: 2026-09-11

## Context

Semantic resolution produces a `BoundObservation` containing a decoded value,
its exact asset-property target, binding provenance, timestamp evidence, and the
bitemporal context used for attribution. History writers, latest-state
projections, and calculators need a downstream contract with an explicit event
time. They must not each silently choose between a device timestamp and host
receipt time.

The platform must also support corrected historical interpretation. Replaying
one decoded observation at a later knowledge cutoff may select a newer binding
revision and a different semantic target. Reusing only the decoded observation
ID as the semantic change ID would collapse those distinct interpretations.

Whether a value may replace latest state requires more information than
semantic compatibility. Source authority, timestamp trust, source quality,
ordering, ties, and freshness are separate policies that have not yet been
defined. The contract must preserve the candidate without deciding those
policies prematurely.

## Decision

Add `AssetPropertyChange`, `AssetPropertyChangeId`, and `EventTimeBasis` to the
`fleetiq-semantics` crate.

An asset-property change owns a successfully bound observation and one explicit
event-time selection. The supported bases are:

- `DeviceRecorded`, which uses the timestamp reported by the device and fails
  when that timestamp is absent;
- `ReceivedAt`, which uses the time at which the host received the complete
  framed message.

The selected instant is stored with its basis. The complete bound observation
remains available, so both original timestamps and all source, decoder,
binding, target, and resolution lineage survive the selection.

The deterministic asset-property change identity contains:

- the decoded observation identity;
- the signal-binding identity;
- the exact signal-binding revision;
- the selected event-time basis.

This identity makes repeated processing of the same semantic interpretation
idempotent while keeping a corrected binding interpretation distinct. A change
represents an observed update candidate. It does not assert that comparison
with previous state found a different value.

## Consequences

- Downstream consumers receive an explicit event time without losing timestamp
  evidence.
- Missing device time cannot silently become an invented timestamp.
- Callers may deliberately use receipt time when device time is absent or
  unsuitable under their policy.
- Corrected binding revisions create distinct semantic change identities while
  retaining the same decoded observation identity.
- History, latest-state, and calculator consumers can share one attributed
  input contract.
- Latest-state eligibility, source selection, timestamp trust, quality
  handling, ordering, tie-breaking, and freshness remain later policy
  decisions.
- Derived calculator results require their own output and identity contract;
  they are not represented as decoded observations.

## Alternatives considered

### Let each downstream consumer choose its own timestamp

This can place one observation at different instants in history, state, and
calculation paths without recording why. It also encourages silent fallback
when device time is missing.

### Always prefer device-recorded time

Some protocols do not provide it, and some device clocks will be missing,
stale, or implausible. Treating device time as universally authoritative would
embed an unsupported operational assumption in the shared model.

### Always use host receipt time

Late uploads and buffered device messages would be placed at ingestion time
rather than when the measurement occurred, weakening historical attribution
and event-time calculations.

### Reuse the decoded observation identity as the change identity

A decoded observation is stable across semantic reprocessing. Binding
corrections can legitimately change its target, so one decoded identity may
have several recorded semantic interpretations.

### Decide whether the change updates latest state in this contract

That decision requires source authority, quality, clock, ordering, and freshness
policies. Embedding a partial rule here would hide unresolved product behavior
inside a foundational value type.
