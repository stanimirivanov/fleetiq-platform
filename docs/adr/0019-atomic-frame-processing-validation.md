# ADR 0019: Validate complete frame-processing results atomically

- Status: accepted
- Date: 2026-09-12

## Context

ADR 0018 defines one `FrameProcessingOutput` as the complete set of effects a
protocol package proposes for an extracted frame. The result may contain an
ordered collection of decoded outputs, a whole-session-state action, and an
ordered collection of replies. Individual host components already enforce a
session-state byte limit and a per-reply byte limit, but applying those
components independently could make an earlier effect visible before a later
one fails validation.

A package could also emit an excessive number of otherwise valid decoded
outputs or replies. A per-reply byte ceiling alone does not prevent many small
replies from consuming excessive host memory. These limits must apply before
trusted transport and ingestion code can observe any package result.

The host cannot use Rust allocation size as a stable decoded-output byte policy.
That size depends on implementation details and does not correspond directly to
the future serialized ABI representation. The isolated-runtime adapter will
need a separate cap on encoded result bytes and bounded decoding of
variable-length fields.

## Decision

Add a `FrameProcessingGate` to the protocol host. It receives explicit,
nonzero per-frame limits for:

- decoded-output count;
- reply count;
- bytes in one reply;
- combined bytes in all replies.

The session's existing `SessionState` continues to own the maximum retained
state bytes because that policy is scoped to the connection rather than one
package result.

For each proposed result, the gate performs the following work without exposing
intermediate values:

1. reject an excessive decoded-output count;
2. reject an excessive reply count;
3. reject excessive combined reply bytes using saturating accumulation;
4. stage every reply through the existing per-reply `ReplyGate` into a local,
   ordered collection;
5. apply the session-state action only after all other validation succeeds;
6. return the ordered decoded outputs and staged replies.

If any step fails, locally staged replies and decoded outputs are dropped and
the prior session state remains unchanged. Applying state is the last fallible
operation, and no later operation can turn a successful state change into an
error. This provides atomicity for in-memory validation and state application;
it does not claim a transaction across transport or durable storage.

Successful results use `AppliedFrameProcessingOutput`. Its replies remain
classified as immediately ready or awaiting durable acceptance. Its debug
representation reports collection counts without telemetry values or reply
bytes.

Return structured failures containing configured and attempted values. A
per-reply failure also records the reply's zero-based package-order index and
retains the underlying `ReplyStagingError` as its error source.

## Consequences

- No decoded output or staged reply reaches later host code when another part
  of the package result violates a limit.
- Session state changes only when the complete in-memory result is accepted.
- Output and reply order survive host validation.
- Aggregate reply bytes cannot evade the frame budget through many individually
  valid replies.
- Values exactly at every configured limit remain valid.
- Operational diagnostics identify the violated dimension, configured limit,
  attempted value, and offending reply index where applicable.
- A deployment must select limits deliberately; the contract supplies no
  production defaults.
- Serialized result size and variable-length decoded values still require
  bounds at the future ABI decoding boundary.

## Alternatives considered

### Apply each effect as it is validated

This could change session state or expose an early reply before a later reply or
collection limit fails. Rolling back externally visible effects would require a
broader transaction that the in-memory host does not own.

### Clone session state and roll it back on failure

Cloning opaque state adds allocation and copying on every processed frame. The
gate can instead stage replies locally and make state application the last
fallible operation.

### Enforce only individual reply limits

Many small replies could remain individually valid while exceeding a reasonable
per-frame memory and transport budget.

### Estimate Rust heap bytes for decoded outputs

Such an estimate would depend on private representations, allocator behavior,
and arbitrary-precision value implementation. It would not provide the stable
wire-level bound needed for an untrusted WebAssembly result.

### Supply permissive default limits

Defaults would hide an operational capacity decision and could be mistaken for
safe production values. Every host must construct explicit nonzero limits.

### Combine validation with durable observation persistence

Persistence, accepted-message assembly, reply release, and transport writes
cross different lifecycle and infrastructure boundaries. This gate establishes
the in-memory validation result those later stages can consume.

## Out of scope

- WebAssembly request and response serialization;
- maximum serialized package-result bytes;
- text length, decimal precision, and other field-level decoding limits;
- accepted-message construction and decoded-observation assembly;
- durable writes or distributed transaction guarantees;
- transport writes and retry behavior;
- releasing durability-gated replies;
- session concurrency and connection lifecycle;
- metrics, tracing, and package-health policy;
- production limit values and tenant-specific overrides.
