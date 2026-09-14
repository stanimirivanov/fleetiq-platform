# ADR 0026: Apply component output through the host processing gate

- Status: accepted
- Date: 2026-09-14

## Context

FleetIQ can invoke an admitted protocol component and convert its processing
result into semantic SDK values. The trusted protocol host separately validates
one complete `FrameProcessingOutput`, stages replies, and applies its proposed
session-state action only when every host limit succeeds. Leaving these APIs
unconnected allows callers to accidentally expose component output before the
host gate or pass one state value to the component and mutate another afterward.

The component runtime is an infrastructure adapter for the host's protocol
policy. The host policy must remain usable without Wasmtime, while the Wasmtime
adapter may depend on and enforce that policy. The integration must also retain
the distinction between component invocation failures and host validation
failures because their operational and package-health consequences differ.

## Decision

Add `WasmSandbox::process_component_frame` to the WebAssembly adapter. It accepts:

- an admitted `CompiledProtocolComponent`;
- the invocation budget;
- the trusted `FrameProcessingGate`;
- the mutable host-owned `SessionState`; and
- an extracted `WireFrame`.

Restrict the raw component processing invocation to the WebAssembly crate. The
host-gated operation is the public complete-frame processing boundary.

The operation builds `FrameProcessingInput` directly from the wire-frame bytes
and the current bytes of that same session-state object. It invokes the component
first and passes a successful semantic proposal immediately to the host gate.
Callers cannot provide separate invocation-state bytes that disagree with the
state the gate may update.

Return `ProcessedProtocolFrame` only after the host gate succeeds. It contains
the accepted decoded outputs and staged replies together with the Wasmtime
resource report. Session state has then applied the package's complete state
action. The method does not expose the intermediate `FrameProcessingOutput`.

Classify failures as `ProtocolFrameProcessingError`:

- `Invocation` retains package rejection, invalid lifted output, resource
  exhaustion, traps, and other component invocation failures;
- `Validation` retains the host gate's structured failure and the completed
  invocation's resource report.

The WebAssembly adapter depends on `fleetiq-protocol-host`. The host crate does
not depend on Wasmtime, preserving the policy-to-adapter dependency direction.

## Consequences

- Component output cannot bypass the existing count, reply-byte, or session-state
  limits through the integrated processing operation.
- The component observes the same pre-operation session state that the gate may
  update.
- Invocation and validation failures preserve prior session state and expose no
  decoded output or staged reply.
- Host validation failures retain component resource usage for metrics and
  package-health policy.
- Successful results preserve decoded-output and reply order and retain reply
  release conditions.
- Accepting a frame remains an in-memory operation. It does not claim atomicity
  across durable observation writes, broker acknowledgement, or transport I/O.
- The Wasmtime adapter now composes trusted host policy, while the policy crate
  remains independent of the execution technology.

## Alternatives considered

### Let callers invoke and gate in separate steps

This exposes an unvalidated semantic proposal and permits callers to forget the
gate, log package-controlled values, or use different session-state values for
the two steps.

### Put Wasmtime orchestration in the host-policy crate

That would make the reusable framing, state, reply, and processing policies
depend on one execution technology. Native conformance fixtures and future
adapters should be able to use those policies without Wasmtime.

### Accept arbitrary frame and state byte slices

Separate slices do not prove that input came from an extracted frame or that the
state presented to the package is the state subsequently updated by the host.
Using `WireFrame` and `SessionState` preserves those ownership boundaries in the
type signature.

### Apply state inside the component adapter before host validation

A later output-count or reply-limit failure would then require rollback. The
existing host gate already stages fallible work and makes state application the
last fallible step.

## Out of scope

- invoking `locate-frame` from a session input buffer;
- consuming the processed frame from a connection buffer;
- assembling trusted accepted-message and decoded-observation provenance;
- durable raw-frame and observation persistence;
- broker publication, acknowledgement, replay, and deduplication;
- releasing staged replies to transport;
- connection concurrency, ownership, fencing, and failure policy;
- package manifests, activation, upgrades, and rollback;
- production processing-limit values and tenant-specific overrides.
