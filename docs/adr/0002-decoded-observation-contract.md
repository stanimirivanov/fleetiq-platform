# ADR 0002: Protocol-neutral decoded observation contract

- Status: accepted
- Date: 2026-09-10

## Context

FleetIQ must ingest telemetry from built-in and custom protocol packages without
allowing vendor field names or runtime technology to leak into semantic assets.
The binding and normalization stages need typed decoder output, stable replay
identity, source units, source quality, timestamp evidence, and the exact decoder
release that interpreted a message.

Device clocks can be absent, inaccurate, or ahead of server receipt time. A
decoder cannot safely choose the authoritative event time. One accepted wire
message can also contain many observations, and the same raw message can be
reinterpreted deliberately with a newer protocol package during recovery or
decoder correction.

## Decision

Introduce `fleetiq-telemetry` as an infrastructure-independent contract crate.
Protocol packages produce `DecodedObservation` values before semantic binding.

A decoded observation contains:

- the tenant established by host-controlled device admission;
- a source device, endpoint, and protocol-neutral signal identity;
- a Boolean, signed integer, arbitrary-precision decimal, or text value;
- an optional source unit, permitted only for numeric values;
- source-reported good, uncertain, or invalid quality;
- an optional device-recorded timestamp and an independent host receipt time;
- a stable identity composed of the accepted raw-message identity, exact
  protocol-package release, and zero-based decoder output index.

Protocol package versions are opaque validated text. This contract therefore
does not require semantic versioning and does not prevent package distribution
from adopting it later.

The durable raw payload remains outside the decoded observation. The accepted
message identity links decoder output back to that record without copying bytes
through every pipeline event.

## Consequences

- Native and future sandboxed protocol packages can target the same output model.
- Replaying one message through the same package release produces deterministic
  observation identities suitable for idempotent writes.
- Reinterpreting a message with another package release produces distinct
  identities and retains decoder provenance.
- Downstream timestamp policy can compare device and receipt evidence without a
  decoder silently substituting one for the other.
- Binding and normalization can reject value-kind or unit incompatibility before
  updating semantic asset state.
- Arbitrary-precision decimal values avoid introducing binary floating-point
  error at the decoder boundary. Runtime hosts must still enforce input and
  output resource limits.
- Serialization schemas, durable message storage, event-time selection, unit
  conversion, and source authority remain later explicit contracts.

## Alternatives considered

### Put decoded observations in the semantic domain crate

The semantic domain owns assets, properties, relationships, and bindings.
Decoder output exists before semantic attribution and has an independent release
and replay lifecycle, so placing it there would blur the protocol/semantic
boundary.

### Use a generated UUID for every decoder output

Random identities would change during replay unless persisted before decoding.
Deriving identity from accepted message, package release, and output position
makes deterministic replay part of the contract.

### Store one selected event timestamp

Choosing device time or receipt time in the decoder would discard evidence and
embed an unverified clock policy in every protocol package.

### Represent decimal values as `f64`

Binary floating point cannot exactly represent many decimal device values and
would introduce avoidable differences before unit conversion or persistence.
