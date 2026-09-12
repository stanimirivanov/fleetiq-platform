# ADR 0017: Admit and bind the WebAssembly probe export

- Status: accepted
- Date: 2026-09-12

## Context

The Wasmtime experiment compiles a core WebAssembly module during admission and
later invokes a host-selected, temporary probe export. Before this decision,
admission did not inspect that export. A module could compile successfully while
omitting the selected name, exporting a memory or another non-function under
that name, or exposing a function with parameters or results. The incompatibility
would surface only after store creation and module instantiation on an invocation
path.

`WasmSandbox::invoke_probe` also accepted an export name independently of
`WasmSandbox::compile`. Even if a caller inspected one entry point during
activation, another name could be selected later. This weakened the meaning of a
successfully admitted `CompiledModule` and repeated a string lookup on every
invocation.

Wasmtime exposes module export types and a `ModuleExport` index that is bound to
the compiled module. The experiment can use these APIs to establish an
admission-time interface invariant without declaring the temporary `() -> ()`
probe to be FleetIQ's stable protocol package ABI. See the Wasmtime
[`Module::get_export` documentation](https://docs.rs/wasmtime/48.0.2/wasmtime/struct.Module.html#method.get_export).

## Decision

Require `WasmSandbox::compile` to receive the host-selected probe export name.
After module size, Wasmtime compilation, import, and table-count checks succeed,
admission inspects that export and requires:

- an export with the selected name;
- an exported function rather than a memory, table, global, or tag;
- zero function parameters;
- zero function results.

Admission returns separate errors for a missing export, a non-function export,
and an incompatible function signature. The signature error records the actual
parameter and result counts. Each error owns the selected export name so it can
outlive the caller's borrowed string and remain useful in activation diagnostics.

After validation, `CompiledModule` stores Wasmtime's module-bound `ModuleExport`
index alongside the compiled module. `invoke_probe` no longer accepts an export
name. It retrieves the previously admitted function by index, confirms its typed
`() -> ()` shape, and calls it under the existing invocation resource limits.

Failure to retrieve the bound index during invocation remains a runtime error.
This is a defensive path for an internal invariant violation rather than a
package-controlled compatibility result.

The selected export name and no-argument signature remain experiment inputs.
They demonstrate admission and invocation mechanics but do not establish the
names, functions, serialization, memory transfer, or versioning of the stable
protocol package ABI.

## Consequences

- A successfully admitted module is known to contain the temporary probe
  function required by the current invocation path.
- Missing exports, wrong export kinds, and incompatible signatures fail during
  admission instead of device-message processing.
- Invocation cannot switch to a different module export after admission.
- `ModuleExport` avoids repeated string lookup and is verified by Wasmtime to
  belong to the instantiated module.
- Error variants preserve the selected export name and actual signature arity
  for package diagnostics.
- `CompiledModule` now represents a compiled module plus its admitted entry
  point, rather than compiled code alone.
- Future stable ABI work must replace or generalize the temporary probe contract
  deliberately.

## Alternatives considered

### Continue validating the export during every invocation

This repeats invariant checks and discovers permanent package incompatibility on
the message-processing path. It also allows different names to be selected for
different invocations.

### Store only the export name

This prevents callers from choosing another name but still repeats string lookup.
Wasmtime already provides a module-bound export index for this purpose.

### Panic if the admitted export index cannot be retrieved

The type and index lookups should agree, but package admission is a public
fallible boundary. Returning a typed admission error avoids adding a panic path
based on an external artifact.

### Fix a FleetIQ export name now

A fixed name would look like a compatibility promise before framing, session
state, replies, decoded outputs, serialization, and memory transfer have been
composed into one invocation design. Keeping the name host-selected preserves
the experimental status.

### Define the complete protocol ABI in this change

The full ABI needs explicit versioning and data-transfer decisions. This change
only moves a known temporary function-shape check to the correct lifecycle
stage.

## Out of scope

- stable package function names and ABI versioning;
- request, response, state, reply, and observation serialization;
- guest linear-memory transfer conventions;
- package manifests and declared ABI compatibility;
- host imports, WASI, and component-model interfaces;
- package activation persistence and lifecycle state;
- signatures, trust roots, distribution, rollback, and caching;
- protocol-specific package implementations.
