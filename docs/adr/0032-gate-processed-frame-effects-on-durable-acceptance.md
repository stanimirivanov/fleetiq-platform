# ADR 0032: Gate processed-frame effects on durable acceptance

- Status: accepted
- Date: 2026-09-14

## Context

A successful protocol session turn produces one exact wire frame, ordered
decoded outputs, and ordered replies classified by their release condition. The
accepted-message log can now durably append the raw frame, but callers still
need to keep every output and durability-gated reply attached to that exact
frame and its trusted session provenance while the append is pending or retried.

The two reply conditions require different visibility. A handshake or control
reply marked `AfterProcessing` may be needed before durable storage completes.
A data acknowledgement marked `AfterDurableAcceptance` must remain opaque until
the accepted-message log succeeds. Decoded outputs also lack trusted tenant,
device, receipt-time, message, and package provenance until they are assembled
with the resulting `AcceptedMessage`.

This orchestration belongs to the application layer and must remain independent
of Wasmtime. Native protocol implementations and future isolated runtimes should
be able to supply the same `WireFrame` and host-validated
`AppliedFrameProcessingOutput`.

## Decision

Add a runtime-neutral durable frame-acceptance flow to the application crate.

`FrameAcceptanceContext` carries trusted host provenance for one frame:

- the message identity assigned before append;
- admitted tenant and communication-device identity;
- immutable protocol-package release pinned to the session; and
- host receipt time for the complete frame.

`PreparedFrameAcceptance::new` consumes that context, the exact `WireFrame`, and
one `AppliedFrameProcessingOutput`. It moves the frame bytes directly into a
`PendingAcceptedMessage`; callers cannot accidentally provide unrelated payload
bytes. It partitions staged replies without changing order within either release
class:

- ready replies remain visible through `PreparedFrameAcceptance` immediately;
- decoded outputs and deferred replies move into `PendingFrameAcceptance` and
  expose counts but not their values or bytes.

`PendingFrameAcceptance::accept` consumes the pending value and calls the
application-owned `AcceptedMessageLog`. On append failure, the error returns the
complete boxed pending value so the caller can retry the same candidate without
reconstructing or losing outputs and deferred replies.

After append succeeds, verify that the returned accepted message exactly matches
the submitted candidate. Although this equality is part of the log port's
contract, checking it at the effect-release boundary prevents a faulty adapter
from attributing outputs or releasing replies against unrelated acceptance
evidence.

Assemble decoded observations only after that check. Assembly adds the accepted
message's trusted tenant, device, receipt time, message identity, and package
release while retaining package-output order. Then consume each deferred reply
through `DeferredReply::release`, which records the accepted-message identity as
durability evidence.

Return `DurablyAcceptedFrame` with the append disposition, accepted message,
assembled observations, and newly released replies. An exact idempotent log
retry produces the same downstream identities while retaining the
`AlreadyPresent` disposition for metrics.

Classify failures as `FrameAcceptanceError`:

- `Append` owns the retryable pending value and the log error;
- `LogContractViolation` owns the pending value and mismatched returned outcome
  while releasing no gated effect;
- `ObservationAssembly` retains the already durable append outcome, original
  decoded outputs, and still-opaque deferred replies for controlled recovery.

Box the large recovery values inside errors. This keeps the routine `Result`
representation small without discarding state required for retry or diagnosis.
Custom `Debug` implementations report identities, payload lengths, and
collection counts without printing wire frames, decoded values, or reply bytes.

## Consequences

- The exact processed frame and its accepted-message payload cannot diverge.
- Processing-gated replies remain available without waiting on storage.
- Decoded outputs cannot cross this boundary with missing or package-controlled
  tenant, device, receipt-time, message, or package provenance.
- Durability-gated replies remain opaque until append succeeds and retain the
  accepted-message ID used as release evidence.
- An ambiguous append failure preserves one complete retryable unit.
- A faulty log implementation cannot release effects against a different
  accepted message.
- Idempotent append retries reproduce stable observation and reply evidence.
- The application orchestration works with any runtime that produces the
  existing host-validated frame-processing types.
- Observation persistence and transport writes remain later explicit effects.

## Alternatives considered

### Let transport code hold separate frame, outputs, and replies

Independent values can be accidentally paired with different admission context
or accepted-message results, especially across asynchronous append retries.

### Hide every reply until durable acceptance

Handshake and control replies may be valid immediately after processing and can
be necessary for protocol progress. Their reviewed release condition should be
preserved.

### Expose decoded outputs while append is pending

They do not yet have trusted accepted-message provenance. Early exposure permits
publication or persistence that cannot be tied to durable raw input.

### Reconstruct the pending value after append failure

Reconstruction risks changing message identity, receipt time, package release,
output ordering, or payload bytes. Returning ownership retains the exact retry.

### Trust every successful log result without comparison

The port requires exact content, but this is the last boundary before external
effects become eligible. Defensive equality prevents one faulty adapter from
misattributing observations or acknowledgements.

### Put this orchestration in the Wasmtime adapter

Durability and trusted provenance are application concerns shared by native and
isolated protocol runtimes. Depending on Wasmtime would reverse that boundary.

## Out of scope

- invoking framing or protocol processing;
- generating accepted-message identities and receipt timestamps;
- retry timing, backoff, circuit breaking, and dead-letter policy;
- persisting or publishing decoded observations;
- writing ready or released replies to transport sockets;
- coordinating observation persistence with accepted-message append;
- replaying already accepted messages through a package;
- connection ownership, routing, backpressure, and failure policy;
- package selection, activation, upgrades, and rollback.
