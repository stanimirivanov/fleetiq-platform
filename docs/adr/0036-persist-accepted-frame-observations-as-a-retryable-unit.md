# ADR 0036: Persist accepted frame observations as a retryable unit

- Status: accepted
- Date: 2026-09-15

## Context

The application acceptance flow now returns `DurablyAcceptedFrame` containing
the accepted raw message, assembled observations, and replies released by
raw-message durability. The decoded-observation log can separately persist a
complete ordered result atomically and idempotently.

Callers still need to compose those boundaries without losing the exact batch,
the raw-message append disposition, or already eligible replies when decoded
persistence fails. An ambiguous append error must permit retry without
reprocessing the protocol frame, appending raw input again, changing output
identity, or reconstructing receipt time.

The reply condition is important: `AfterDurableAcceptance` refers to raw-message
acceptance. A later decoded-result append must not implicitly delay or revoke
an already satisfied release condition. Conversely, downstream semantic binding
or publication should consume the complete durable observation result rather
than a pending batch.

## Decision

Add `DurablyAcceptedFrame::prepare_observation_persistence` as an explicit
application transition.

Validate the frame's borrowed accepted message and observations against
`DecodedObservationBatch` invariants before moving ownership. Add a borrowed
batch validation operation so an unexpected internal invariant failure can
return the original complete frame. After validation, move the accepted
message, observations, and replies into `PendingObservationPersistence` without
cloning the payload or observation collection.

Preserve whether raw acceptance returned `Appended` or `AlreadyPresent` as the
`accepted_message_was_appended` disposition. The accepted message is owned once
by the observation batch rather than duplicated inside another append outcome.

The pending value exposes:

- already durable accepted-message evidence;
- raw-message append disposition;
- observation count; and
- already released replies with their existing acceptance evidence.

It does not expose decoded values. Its `persist` operation borrows the complete
batch for `DecodedObservationLog::append` and consumes the pending value only
after the outcome is known.

On append failure, return `ObservationPersistenceError::Append` with the log
error and the complete boxed pending value. Retrying that value performs only
the decoded-result append. It retains message identity, package release,
observation order and content, receipt-time evidence, and released replies.

On success, return `DurablyPersistedFrame` with:

- raw-message append disposition;
- decoded-result append disposition;
- accepted raw message and complete durable observation batch; and
- the same released replies in package order.

Both `Appended` and `AlreadyPresent` observation outcomes authorize this
completed result. Empty batches use the same transition and log operation;
they are not skipped as if there were no work.

An unexpected preparation invariant failure returns
`ObservationPersistencePreparationError`, which retains the complete durably
accepted frame for diagnosis. Supported acceptance-flow values satisfy the
batch invariant by construction.

Custom debug output reports identities, append dispositions, and collection
counts without printing decoded values, raw frames, or reply bytes.

## Consequences

- Accepted-frame effects cross decoded-result durability as one owned unit.
- Ambiguous append failures remain exactly retryable.
- Observation retries do not re-run protocol processing or raw-message append.
- Pending observation values are unavailable through the public pending API.
- Successful completion exposes one complete durable batch for later semantic
  work or publication.
- Zero-output decoding receives the same durable completion treatment.
- Raw and decoded append dispositions remain separately visible for metrics.
- Payload and observation ownership moves without cloning at preparation.
- Already released replies remain eligible independently of observation
  persistence.
- The application does not perform transport writes or decide whether a reply
  was already sent; that remains the transport owner's responsibility.
- The orchestration depends on application ports and host-neutral types rather
  than PostgreSQL or Wasmtime.

## Alternatives considered

### Let each caller construct and retain a batch independently

Repeated orchestration risks losing append disposition, replies, or exact retry
content between asynchronous calls.

### Clone the accepted message into the batch while retaining the full raw append outcome

This duplicates the wire payload during an ingestion transition. Keeping the
raw append disposition separately allows ownership to move once.

### Retry from the original protocol frame after observation append failure

Reprocessing may repeat protocol effects or change message identity, receipt
time, output ordering, or package selection. The existing complete batch is the
correct retry unit.

### Hide released replies until decoded-result persistence succeeds

This strengthens the reviewed reply condition beyond raw-message durability
and can block protocol progress unnecessarily.

### Expose pending observations for downstream work

Callers could publish or bind values whose complete decoded result is not yet
durable. The completed result is the intended downstream boundary.

### Skip persistence for empty observation collections

That would discard explicit completion evidence for successful zero-output
decoding.

## Out of scope

- combining raw-message and decoded-result appends into one transaction;
- automatic retries, retry timing, backoff, or dead-letter handling;
- cancellation-safe background ownership of pending work;
- transport writes and reply-delivery deduplication;
- semantic signal binding and asset-property change persistence;
- publishing observations or updating live projections;
- replay scheduling and decoded-result query APIs; and
- connection ownership, admission, routing, and backpressure.
