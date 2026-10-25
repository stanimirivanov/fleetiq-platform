# ADR 0111: Add an opt-in protocol identity handoff

## Status

Proposed, 25 October 2026.

The owner approved the generated-ABI lint exception in this implementation
session; the optional identity-world decision awaits PR review.

Supersedes: none. Superseded by: none.

## Context

Independent guest authors need an exercised build workflow and a way to report
wire-level external identity evidence before a host can route a new connection.
Changing the existing runtime result would break admitted 0.1.0 components.

## Decision

Keep the existing runtime WIT interface unchanged. Add a named, opt-in
`identity-protocol-package` world in the same versioned package, including the
original world plus an identification interface. Bind it with a separate explicit
host admission API. Legacy admission and invocation remain available.

Identification receives bounded payload/state bytes in a fresh bounded store and
returns exactly one kind/value claim or a bounded error code. SDK validation
limits kind to 64 ASCII identifier bytes and value to 128 UTF-8 bytes without
control characters; diagnostics redact the value. No tenant, internal device ID,
credential, routing decision or execution authority is returned.

The host must independently resolve and authorize claims. Identification has no
session-state effects and releases no replies. A later commissioning/routing
slice owns using this evidence; this decision does not grant devices access.

Use maintained Bytecode Alliance wit-bindgen for Rust guest glue and wit-component
for componentization. Keep the example in the governed workspace with a pinned
compiler target and lockfile. Conformance compiles actual source and exercises the
result in the ordinary host rather than substituting a hand-written WAT fixture.

## Consequences

Existing components remain compatible. Identity-capable packages have an explicit
additional contract and must be selected deliberately. The host performs input
and semantic validation under its existing execution/memory limits; compilation
is still outside invocation budgets. Native integration tests require the pinned
Wasm target. Build inputs are reproducible; cross-host byte identity remains a
publication qualification responsibility.

## Alternatives considered

- Change frame-processing-output: breaks the existing interface and couples
  pre-admission identity evidence to already-attributed processing.
- Interpret opaque session bytes as identity: creates an undocumented ABI.
- Let packages choose tenant/device IDs: crosses the trusted-host boundary.
- Hand-code canonical ABI glue: duplicates maintained ecosystem generators.

The owner explicitly approved generated ABI glue in issue #315's implementation
session. Workspace unsafe remains deny; the single generated export module has
a mechanically enforced allowance. A Syn-based architecture sensor rejects
other unsafe allowances and additions to the fixed generator/export boundary.

The owner also approved only `clippy::same_length_and_capacity` on that generated
module: canonical ABI allocations have capacity equal to length. The sensor
rejects extra allowances there; handwritten code keeps the full Clippy policy.
