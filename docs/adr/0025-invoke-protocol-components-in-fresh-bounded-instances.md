# ADR 0025: Invoke protocol components in fresh bounded instances

- Status: accepted
- Date: 2026-09-13

## Context

FleetIQ admits protocol components against the exact versioned WIT world and
retains generated pre-instantiation bindings. The runtime now needs to call the
typed framing and processing operations without allowing guest memory, globals,
or accidental ambient state to cross invocation boundaries.

The existing sandbox already enforces fuel, an epoch deadline, an approximate
guest stack ceiling, linear-memory growth, and table growth for a store. Those
limits must cover component instantiation as well as the exported operation.
Component Model calls also have three distinct failure classes: Wasmtime can
fail, the package can reject protocol input with its stable code, or structurally
valid WIT output can violate the semantic SDK contract.

A componentized WIT guest instantiates both the package core module and the
canonical ABI adapter. The one-instance resource limit used by the earlier core
module experiment therefore rejects every valid component before execution.

## Decision

Add typed `WasmSandbox` operations for framing and frame processing. Each call:

1. copies the borrowed semantic input into its generated WIT value;
2. creates a new store with the caller's `InvocationBudget`;
3. instantiates the admitted `ProtocolPackagePre` bindings in that store;
4. calls the generated `locate-frame` or `process-frame` binding;
5. completes resource accounting through the existing invocation classifier;
6. separates a package-declared rejection from a Wasmtime failure; and
7. converts successful WIT output through the semantic SDK constructors before
   returning it.

Return `ProtocolComponentOutput<T>` with the validated semantic value and
`InvocationReport`. Package rejection and invalid semantic output also retain
the report because guest execution completed successfully and its resource use
is available. Runtime failures continue to use `InvocationError`, preserving
fuel, deadline, stack, memory, table, and residual Wasmtime classifications.

Create a fresh store and component instance for every operation. Package-owned
continuity is carried only by the explicit session-state input and the proposed
session-state action. The host remains responsible for committing processing
outputs and releasing replies.

Raise the store's explicit instance allowance from one to two. This admits the
package module and canonical ABI adapter while preserving one guest linear
memory and one guest table as separate limits.

## Consequences

- Admitted packages can execute both WIT operations through typed bindings.
- Instantiation and execution share one fuel, deadline, stack, memory, and table
  budget.
- Guest globals and linear memory are discarded after every success or failure.
- Session continuity is visible, serializable, and controlled by the host.
- Package protocol errors remain distinguishable from traps and host resource
  enforcement.
- Structurally valid but semantically invalid component values never enter the
  protocol host.
- Successful calls and completed package or validation failures expose consumed
  fuel for metrics and policy.
- Component invocation allocates and instantiates per call; measurements may
  later justify pooling only with an explicit reset and isolation contract.

## Alternatives considered

### Reuse one component instance across calls

This reduces instantiation work but lets guest globals and memory become hidden
session state. Safe reuse would require ownership, reset, cleanup, poisoning,
and concurrency rules that the current package contract does not define.

### Treat package rejection as a Wasmtime failure

A package error is a normal typed result describing protocol input. Combining it
with traps would prevent callers from applying different session and telemetry
policies and would discard its stable code.

### Return generated WIT values to the protocol host

Canonical ABI lifting verifies memory safety and WIT shape, but it cannot enforce
SDK invariants such as nonzero frame lengths, valid identifiers and decimals,
compatible values and units, or nonempty replies.

### Keep the one-instance resource limit

A Component Model package includes an adapter instance in addition to its core
module. Keeping the earlier limit makes every admitted protocol component
impossible to invoke.

## Out of scope

- applying processing-output count and aggregate-byte limits;
- atomically committing decoded outputs, session state, and replies;
- transport buffering, frame consumption, and connection-session orchestration;
- package manifests, signatures, provenance, activation, and rollout;
- compilation worker isolation and activation concurrency;
- component pooling, reuse, snapshots, and reset protocols;
- WASI or other host capability imports;
- protocol-specific package implementations and guest SDK publication.
