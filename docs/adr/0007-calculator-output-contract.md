# ADR 0007: Calculator output contract

- Status: accepted
- Date: 2026-09-11

## Context

FleetIQ calculators will consume semantically attributed asset changes and may
combine observations across devices, properties, components, and event-time
windows. Their outputs must retain the exact calculator implementation,
evaluation scope, logical time, and replay identity without depending on a
specific worker runtime or anomaly algorithm.

Protocol decoding currently defines `DecodedValue`, but calculated results are
not decoded device facts. Maintaining a second Boolean, integer, decimal, and
text representation for analytics would create conversion code and allow the
two models to drift.

One calculator evaluation may emit several derived properties or no properties.
Retries must reproduce the same output identities. Large event-time windows may
contain many source observations, so copying every evidence identifier into
every output would increase event and storage size substantially.

## Decision

Move the shared Boolean, integer, decimal, and text representation into the
domain crate as `PropertyValue`. Keep `DecodedValue` as the protocol-facing type
alias so telemetry APIs retain their domain vocabulary while analytics uses the
general semantic name.

Add a `fleetiq-analytics` crate with validated identifiers for:

- a stable calculator;
- an immutable calculator version;
- one logical calculator evaluation.

A `CalculatorEvaluation` is host-owned context containing:

- evaluation identity;
- tenant identity;
- the asset that scopes input selection and calculator state;
- the exact calculator ID and version;
- the logical event-time instant or window boundary;
- the time at which the host performed the evaluation.

The orchestrator must reuse the evaluation identity when replaying the same
calculator release, scope, configuration, logical trigger, and input selection.
That identity links to the durable evaluation record and its input evidence;
the output contract does not copy an unbounded evidence list.

Calculator logic emits ordered `CalculatedProperty` values. Each value contains
a target asset, exact property-definition reference, property value, and
canonical unit. Construction checks the value kind and unit against the exact
property definition.

The host assembles final `CalculatorOutput` values. Each deterministic output ID
contains the evaluation identity, exact calculator release, and zero-based
output position. The same evaluation must emit equivalent outputs in the same
order. A calculator may emit no outputs.

The target asset may differ from the scope asset. This supports calculations
that read child components and publish a result on an assembly, or read an
assembly and publish a result on a specific component, when allowed by the
calculator definition and semantic model.

## Consequences

- Decoded and calculated data share one semantic property-value representation.
- Calculator output cannot silently use a value kind or unit incompatible with
  its exact target property definition.
- Calculator identity and version remain visible on every output.
- Event time remains separate from processing time for late data and replay.
- Multi-output evaluations have deterministic identities when output order is
  deterministic.
- Evaluation identity provides a bounded link to configuration and input
  evidence instead of duplicating large windows in output events.
- Tenant authorization, target eligibility, and scope rules remain host and
  application responsibilities.
- Calculator definitions, declared inputs, state, windowing, late-data policy,
  checkpoints, evidence storage, confidence, and algorithms remain later work.

## Alternatives considered

### Reuse `DecodedValue` directly in analytics

The representation is suitable, but the name incorrectly describes calculated
results as decoded device data. A domain-level value with a protocol-facing
alias preserves terminology at both boundaries.

### Define a separate calculated-value enum

Duplicating the same storage kinds would require conversions and parallel
validation as new value kinds are introduced.

### Let calculator logic construct complete outputs

This lets calculation code choose tenant, evaluation, processing time, and
calculator-version provenance. Host-controlled assembly keeps orchestration
facts outside algorithm implementations.

### Generate a random identity for every output attempt

Retries would create duplicate derived facts. Evaluation identity plus stable
output position provides deterministic replay identity.

### Embed all input observation IDs in every output

Windowed calculations may consume thousands of values and produce several
outputs. Repeating the full evidence list increases broker and storage costs.
A durable evaluation record can own configuration and evidence once and remain
addressable through the evaluation ID.

### Require the target asset to equal the scope asset

Aggregate and subsystem calculations may intentionally publish across an asset
hierarchy. Calculator definition validation should control allowed targets
rather than imposing equality in the generic output contract.
