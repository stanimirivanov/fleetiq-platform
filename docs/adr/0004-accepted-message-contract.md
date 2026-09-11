# ADR 0004: Durably accepted protocol message contract

- Status: accepted
- Date: 2026-09-11

## Context

FleetIQ has protocol-neutral decoded and semantically bound observation
contracts, but it needs an explicit input boundary for replayable protocol
decoding. A transport session can receive fragmented bytes, and a protocol
package can frame one message that later produces observations from several
endpoint-scoped sources. The host must retain that framed input before releasing
any protocol data acknowledgement whose meaning is durable acceptance.

Device admission establishes tenant and communication-device identity. The
session also pins the protocol-package release that owns framing and decoding.
These facts must survive retries so a decoder does not depend on mutable session
configuration when a message is replayed.

The platform has not yet selected the durable log, serialization format, or
complete acknowledgement guarantee. The contract must describe the accepted
fact without coupling it to those infrastructure choices.

## Decision

Add `AcceptedMessage` and `AcceptedPayload` to the infrastructure-independent
`fleetiq-telemetry` crate.

An accepted message contains:

- a stable host-assigned accepted-message identity;
- the tenant established by device admission;
- the admitted communication-device asset identity;
- the immutable protocol-package release pinned to the session;
- the time at which the host received the complete framed message;
- immutable, uninterpreted wire bytes.

The host exposes the accepted-message value to decoding only after its configured
durability condition succeeds. The Rust constructor represents this contract but
cannot prove external storage durability; ingestion orchestration must enforce
that condition.

The accepted message does not contain a source endpoint or signal. Framing
precedes decoding, and one message may yield observations for several endpoints
or relayed systems. Each `DecodedObservation` supplies its own source and derives
stable replay identity from the accepted-message ID, protocol-package release,
and output index.

`AcceptedPayload` deliberately permits empty frames because custom protocol
semantics belong to the package. Host-configured framing and resource limits
remain responsible for rejecting unsupported or oversized input. Its `Debug`
implementation reports payload length without printing wire bytes.

## Consequences

- Native and sandboxed protocol runtimes can consume the same accepted input.
- Replay retains admission identity, receipt time, package provenance, and exact
  bytes independently of a live connection.
- A package can emit several endpoint-scoped observations without assigning an
  endpoint prematurely to the raw frame.
- Routine debug output avoids copying device payload contents into logs.
- The durability mechanism, message serialization, acknowledgement guarantee,
  encryption, retention, and payload-reference strategy remain explicit later
  decisions.
- Large payloads are currently represented in memory. A later persistence or
  runtime boundary may supply bounded buffers or references while preserving the
  accepted-message semantics.

## Alternatives considered

### Pass raw byte slices directly from the transport to the decoder

Borrowed session buffers do not provide durable replay identity or preserve the
admission and package context required after reconnects and worker failures.

### Put endpoint identity on the accepted message

The framed wire message may contain records from multiple buses, peripherals, or
logical endpoints. Assigning one endpoint before decoding would misrepresent
those protocols.

### Include a broker sequence or storage location in the core contract

Broker offsets and object locations are infrastructure coordinates. They may be
recorded by an envelope or persistence adapter without becoming the identity or
meaning of an accepted protocol message.

### Reject empty payloads in the contract

Some custom protocols may define an empty frame as meaningful. The protocol
package and host resource policy can reject it when unsupported; the shared
contract should not make that protocol decision globally.
