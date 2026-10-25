# ADR 0114: Add a bounded Teltonika protocol component

## Status

Proposed, 25 October 2026.

Supersedes: none. Superseded by: none.

## Context

The guest authoring boundary, artifact admission and registered-device routing
exist, but no vendor decoder has been exercised as a maintained component.
The first implementation must preserve source evidence without embedding
business-asset semantics, opening a listener or assuming an IMEI authenticates a
peer. Its output vocabulary becomes an input to future versioned bindings.

## Decision

Implement a separate `fleetiq-teltonika` adapter package with a pure checked wire
parser and thin WIT conversion boundary. Use the unchanged identity world and
pinned wit-bindgen version already exercised by the custom example. There is no
new third-party dependency family and no host/vendor coupling in inner crates.

Support the documented TCP IMEI handshake and Codec 8/8 Extended subset. Bounds,
source signal names, state encoding, error codes and build procedure belong to
[the package support contract](../../packages/teltonika/README.md). Validate the
whole frame, CRC and all records before proposing any output or acknowledgement.
Keep exact device time, integer coordinate scale and unknown IO bytes. Preserve
raw altitude bits pending a device-specific interpretation. Raw hexadecimal IO
text preserves signedness choices and all unsigned 64-bit patterns losslessly.

The handshake proposes an immediate ACK only after the trusted host has resolved
and validated a registered route. The component cannot establish that authority.
AVL counts propose replies gated on raw-message durable acceptance, independently
of later decoded-result completion. A one-byte versioned session marker carries
protocol sequencing only, without retaining the IMEI or host identity in state.

The owner approved the same exact generated-ABI allowance for the fixed
`packages/teltonika/src/component/bindings.rs` module in issue #321. Expand the
sensor's exact owner/file set to two; do not introduce directory-pattern
exceptions. Generated files must still contain only the reviewed generator and
export statements. All handwritten Rust retains unsafe denial and strict Clippy.

## Consequences

The package is usable for source-built conformance and later admission, but this
is not evidence of listener, hardware, replay-worker or live deployment readiness.
A later source-contract change needs a new immutable package release and explicit
mapping review. The backend image context includes this workspace member so
existing Cargo metadata/build operations remain valid; the runtime does not
link or automatically load the guest.

## Alternatives considered

- Decode in server/application code: rejected because it couples vendor parsing
  to trusted transport and business policy.
- Guess model-specific IO units from AVL IDs: rejected because model/firmware
  semantics can differ and raw evidence would be lost.
- Handwrite the canonical Wasm ABI: rejected in favour of the existing generated,
  mechanically constrained boundary.
- Claim full Teltonika compatibility: rejected; the bounded subset and exclusions
  must remain visible until later hardware qualification.
