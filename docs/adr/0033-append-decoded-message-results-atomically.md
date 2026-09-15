# ADR 0033: Append decoded message results atomically

- Status: accepted
- Date: 2026-09-14

## Context

After raw message acceptance, the application assembles zero or more decoded
observations with trusted tenant, communication-device, receipt-time, message,
and protocol-package provenance. These outputs need a separate durability
boundary before semantic binding, publication, or replay progress can depend on
them.

Persisting each observation independently would make partial batches visible
after a failure. It would also provide no durable evidence for a valid message
that decodes successfully to zero observations. Counting observation rows
therefore cannot distinguish completed empty decoding from work that has not
run or has failed.

Retries may follow an ambiguous storage error. They must neither duplicate
observations nor accept another result under the same decoding identity.
Package output order is significant because the zero-based position is part of
each observation's deterministic replay identity.

## Decision

Define `DecodedObservationBatch` in the application layer as one complete
decoded-message result. It owns the durably accepted message and its ordered
observations. Construction verifies every observation against host-trusted
message evidence:

- collection position equals the observation's zero-based output index;
- tenant equals the admitted tenant;
- source communication device equals the admitted device;
- accepted-message identity equals the decoded message;
- decoder provenance equals the exact package release pinned to the message;
  and
- host receipt time equals the accepted message's receipt time.

An empty observation collection is valid and represents completed decoding
with no telemetry output. The batch's custom `Debug` representation reports the
accepted message and observation count without printing decoded values.

Define an application-owned `DecodedObservationLog` port. Implementations append
one message-level result marker and all child observations atomically. The
batch identity consists of tenant, accepted-message ID, and immutable
protocol-package release. `Ok` means the configured durability condition has
succeeded for the complete result.

An exact retry returns `AlreadyPresent`. Reusing the batch identity with a
different observation count, order, or immutable content is an identity
collision and fails. An error exposes no partial success and may be ambiguous
about durability; because `append` borrows the batch, the caller retains the
exact value for retry.

Keep the port independent of PostgreSQL row types and transaction APIs.
Infrastructure adapters own result-marker representation, atomic insertion,
content comparison, and database error classification.

## Consequences

- Zero-output decoding has explicit durable completion evidence.
- Consumers never observe part of a decoded message's output as a successful
  append.
- Observation order and deterministic output identities remain aligned.
- Trusted message provenance cannot be mixed across a persisted batch.
- Ambiguous failures can be retried with the exact same value.
- Storage adapters must compare all immutable batch and observation content
  before classifying a conflict as an exact retry.
- The accepted raw message and decoded result remain separate durability facts,
  allowing replay and operational diagnosis without coupling protocol decoding
  to semantic attribution.

## Alternatives considered

### Append each observation independently

A failure can leave a prefix visible, and a message producing no observations
has no completion record.

### Infer completion from observation rows

No row is ambiguous between a valid empty result, unfinished work, and failure.

### Publish observations before durable batch completion

Downstream consumers could act on output that storage cannot later reproduce as
one complete decoding result.

### Use only the accepted-message ID as retry evidence

Key equality does not prove that count, order, provenance, timestamps, sources,
measurements, and quality match the previously stored result.

### Put the persistence trait in the PostgreSQL crate

The application consumes the durability capability and owns its semantics.
Keeping the port inward prevents database types and transaction choices from
entering orchestration or telemetry contracts.

## Out of scope

- the PostgreSQL decoded-result schema and migration;
- a PostgreSQL implementation of `DecodedObservationLog`;
- coordinating accepted-message append and decoded-result append in one
  database transaction;
- retry timing, backoff, dead-letter handling, and replay scheduling;
- semantic signal binding and asset-property change persistence;
- observation publication, query APIs, retention, and archival;
- re-decoding an accepted message with a different package release; and
- comparing floating-point payload encodings or defining database value
  serialization.
