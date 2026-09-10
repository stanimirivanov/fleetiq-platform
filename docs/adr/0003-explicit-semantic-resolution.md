# ADR 0003: Explicit and conservative semantic resolution

- Status: accepted
- Date: 2026-09-10

## Context

Decoded observations identify a communication device, endpoint, and source
signal but do not yet identify the semantic asset property being observed.
Signal bindings provide that attribution with effective and recorded time, while
property definitions provide the expected value kind and canonical unit.

Resolution must handle late commissioning data and corrections. It must also
avoid silently choosing among competing bindings, converting units without a
declared rule, or losing observations that cannot currently be attributed.

## Decision

Introduce `fleetiq-semantics` with a `SignalBindingResolver`. The resolver owns
focused read-view traits for bitemporal binding lookup and exact property lookup.
Blanket implementations adapt the existing application repositories, while a
worker-local projection can implement only the reads it serves.

Every resolution call supplies two explicit instants:

- effective time selects the binding that applied in the physical world;
- knowledge time selects the binding revision visible to that interpretation.

The caller selects effective time. The resolver does not choose between the
device-recorded timestamp and host receipt time.

Resolution produces a bound observation only when:

- exactly one binding selects the decoded source in the requested snapshot;
- the binding's exact property definition is available;
- decoded and property value kinds are equal;
- source and canonical property units are equal, including both being absent.

The bound result retains the complete decoded observation, semantic target,
binding identity and revision, and bitemporal resolution context.

Expected configuration and compatibility failures produce an unresolved result
that retains the complete decoded observation. Reasons distinguish no binding,
competing bindings, missing definitions, value-kind mismatch, and required unit
conversion. Candidate bindings are ordered deterministically for diagnostics.

Repository failures remain typed operational errors rather than unresolved data.

## Consequences

- Delayed observations can be resolved against the physical configuration that
  applied at their selected event time.
- Replay can reproduce an earlier interpretation or request a corrected view by
  changing knowledge time explicitly.
- Missing configuration does not discard decoded telemetry.
- Multiple effective bindings do not create an implicit last-write-wins source.
- A conversion must be configured before differently unitized data can update a
  canonical asset property.
- Source-reported invalid or uncertain quality can still be attributed and
  preserved; a later state policy decides whether it can update current state.
- Repository implementations can be PostgreSQL adapters, local projections, or
  test views without changing resolver behavior.
- The resolver does not depend on persistence write operations or unrelated
  semantic-definition queries.

## Alternatives considered

### Select the first returned binding

Repository order is not an authority policy. Selecting the first result would
make ambiguous configuration appear valid and could route data to the wrong
asset.

### Resolve using the current binding only

Current-state lookup would misattribute delayed telemetry after equipment or
commissioning changes and would make historical replay non-reproducible.

### Use receipt time when device time is absent or suspicious

That may become a valid configured timestamp policy, but embedding it in binding
resolution would mix clock assessment with semantic attribution and hide the
decision from provenance.

### Convert common units automatically

Even familiar unit pairs require an explicit, versioned conversion contract and
failure behavior. Exact equality is the only conversion-free rule in this
increment.

### Return configuration problems as operational errors

Missing and ambiguous bindings are expected commissioning states. Treating them
as repository failures would encourage retries without diagnostics and would not
retain a structured reason for operator correction.
