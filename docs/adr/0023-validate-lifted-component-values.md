# ADR 0023: Validate lifted component values at the semantic boundary

- Status: accepted
- Date: 2026-09-13

## Context

The versioned protocol-package WIT world gives Wasmtime enough type information
to lift guest results into generated Rust records and variants. Successful
canonical ABI lifting proves that the guest supplied structurally valid WIT
values. It does not prove FleetIQ domain invariants such as nonempty frame
lengths, valid endpoint identifiers, exact decimal syntax, numeric-only units,
or nonempty transport replies.

Generated WIT types should not spread into the semantic SDK or protocol host.
Doing so would couple stable domain behavior to Wasmtime code generation and
could let later code accidentally treat untrusted component values as validated
application data.

Conversion errors also cross an observability boundary. Device payloads,
decoded text, identifiers, and replies can contain sensitive or high-cardinality
values. Routine errors need enough location information to diagnose a package
contract violation without retaining those values.

## Decision

Add a private conversion layer beside the generated component bindings. It owns
both directions across the typed component boundary:

- borrowed semantic framing input becomes an owned WIT invocation value whose
  payload is the complete unread transport buffer;
- borrowed semantic processing input becomes an owned WIT invocation value
  whose payload is exactly one complete frame;
- session-state bytes are copied unchanged in both cases;
- a WIT frame decision becomes the SDK `FrameBoundary` only after conversion to
  a nonzero host `usize`;
- a WIT processing result becomes the SDK `FrameProcessingOutput` only after
  every nested value is valid.

Construct endpoint, signal, and unit identifiers through their domain
constructors. Parse decimal strings into the domain's arbitrary-precision
decimal representation. Construct `DecodedMeasurement` to enforce that units
occur only on numeric values, and construct every `ProposedReply` to reject
empty transport writes. Preserve emitted collection order, device timestamps,
quality, complete state actions, and reply release conditions exactly.

Return a typed `ComponentOutputError` internally for expected contract
violations. Errors identify a decoded-output or reply with its zero-based
position and retain trusted invariant error types where useful. Invalid decimal
errors report only the position because a parser error can retain or display
package-controlled text. No conversion error stores the rejected identifier,
decimal, or reply. Keep the type crate-private until a public component
invocation error needs to expose it.

Keep the generated WIT types and the conversion implementations private to the
WebAssembly adapter. Later component invocation code can use the conversions
without exposing Wasmtime-generated types as public FleetIQ API.

## Consequences

- Structurally valid WIT output cannot bypass existing FleetIQ value invariants.
- The semantic SDK remains independent from Wasmtime and generated bindings.
- Invalid nested output reports its stable package-emission position.
- Routine conversion diagnostics do not contain package-controlled telemetry or
  reply bytes.
- Conversion stops at the first invalid value and exposes no partial semantic
  processing result.
- Input lowering allocates owned copies because generated component values own
  their byte lists. The canonical ABI then copies those values into guest memory.
- WIT lifting allocates generated lists before this conversion runs. Guest-memory
  limits bound their source representation; configured semantic collection and
  byte limits remain enforced by the host gates after conversion.
- Package-declared numeric errors, traps, resource failures, and admission errors
  remain separate from invalid successful output.

## Alternatives considered

### Let the protocol host consume generated WIT types directly

This would couple host policy to Wasmtime code generation and duplicate domain
validation wherever values are used. A single adapter keeps the trust boundary
explicit.

### Add validation rules to the WIT document

WIT describes data shape and cannot express the full identifier, decimal,
measurement, or reply invariants. Those rules already exist in Rust domain
constructors and should have one implementation.

### Normalize invalid identifiers or decimal text

Silent trimming or rewriting would hide package defects and could change stable
signal identity. Invalid package output is rejected without mutation.

### Include rejected values in conversion errors

That would make diagnostics easier for a single fixture but could leak telemetry
or create unbounded log cardinality in production. Package conformance tooling
can inspect fixtures in an explicitly controlled environment.

### Return valid values before encountering a later failure

Partial conversion would violate the existing atomic frame-processing contract.
The adapter returns one complete semantic result or one error.

## Out of scope

- component admission and embedded-world compatibility checks;
- component instantiation and function invocation;
- pre-lift host allocation limits inside Wasmtime's canonical ABI;
- applying semantic collection, session-state, and reply-byte limits;
- package-declared error-code interpretation;
- trap and resource-failure classification;
- guest SDK generation and conformance fixtures;
- package manifests, activation, version negotiation, and rollback;
- component pooling and lifecycle policy;
- protocol-specific package implementations.
