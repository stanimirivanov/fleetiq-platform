# ADR 0020: Prove bounded opaque WebAssembly payload transfer

- Status: accepted
- Date: 2026-09-12

## Context

The Wasmtime experiment already admits untrusted core WebAssembly modules and
bounds fuel, elapsed time, guest stack, linear memory, and table elements. Its
temporary no-argument probe proves interruption but cannot carry the semantic
framing and processing contracts defined in the protocol SDK.

Before choosing serialization, FleetIQ needs to prove that the host can move an
opaque request into guest memory and copy an opaque response out without trusting
guest-provided addresses or lengths. That mechanism must not let a package make
the host allocate from an unchecked result length, read or write outside current
linear memory, select different exports after admission, or require a fallible
guest cleanup call after processing.

This experiment uses core WebAssembly because the current sandbox is already
built around core modules. The names and function shapes are implementation
inputs rather than the stable protocol-package ABI.

## Decision

Add a second admission and invocation path alongside the existing probe. During
admission, the host selects and binds three module exports:

- one core WebAssembly memory;
- an allocator with signature `(i32) -> i32`;
- a processor with signature `(i32, i32) -> i64`.

The allocator receives the opaque input length and returns an input pointer. The
processor receives that pointer and length. Its `i64` result is an unsigned
descriptor with the output pointer in the high 32 bits and output length in the
low 32 bits. Pointers and lengths use their complete unsigned 32-bit bit
patterns even though core WebAssembly function types expose `i32` and `i64`.

Admission verifies each selected export's existence, external kind, and exact
function signature. The compiled transfer artifact retains module-bound export
indices, preventing invocation from changing names or repeating string lookup.

Each invocation receives explicit nonzero 32-bit input and output byte limits.
The host:

1. rejects oversized input before creating a store or calling guest code;
2. creates a fresh store and instance under the existing invocation budget;
3. calls the guest allocator;
4. validates the complete unsigned input range against current memory;
5. copies input with Wasmtime's safe memory API;
6. calls the guest processor;
7. rejects an oversized reported output before host allocation or memory read;
8. validates the complete unsigned output range against current memory;
9. reserves a bounded host buffer fallibly and copies the output;
10. returns the opaque bytes with the invocation fuel report.

A range is valid when its half-open interval `[offset, offset + length)` fits in
the current linear-memory size. Arithmetic is widened before addition. An empty
range may point exactly one byte past the last address, which is the end of the
memory interval.

Instantiation, allocation, and processing remain subject to fuel, epoch,
linear-memory, table, and stack limits. Existing resource-limit classification
is shared by probe and transfer paths.

Do not require a guest deallocator. Every invocation owns a fresh store and
instance, and dropping them reclaims all guest allocation on success and every
failure path. Calling untrusted cleanup after a trap could consume exhausted
fuel, exceed the deadline, or introduce another failure without improving
reclamation.

Routine debug output reports response length and resource usage without opaque
payload bytes. Host code contains no `unsafe` memory access.

## Consequences

- The host can exchange arbitrary bounded bytes without committing to their
  schema or serialization format.
- Guest pointers and lengths never become host slices before complete range
  validation.
- Guest output length is checked before allocating the host response buffer.
- Empty requests and responses are supported explicitly.
- Export incompatibility fails during admission rather than message processing.
- The same fuel and resource classification applies to probe and transfer
  execution.
- Fresh instances give deterministic allocation cleanup and prevent mutable
  guest state from leaking between invocations.
- Per-invocation instantiation has a performance cost that must be measured
  before choosing the production isolation and pooling model.
- A future stable ABI can replace the host-selected names and opaque bytes while
  retaining the tested transfer invariants.

## Alternatives considered

### Choose protocol serialization in the same change

Serialization determines compatibility, schema evolution, malformed-field
handling, and SDK ergonomics. Separating byte transfer lets those decisions be
reviewed without mixing them with guest-memory safety.

### Use fixed guest scratch addresses

Fixed addresses can overlap guest data, stacks, or allocator state and constrain
toolchains. A guest allocator lets the compiled package select valid storage,
subject to host range and memory limits.

### Let the guest call a host function with its result

The current runtime intentionally exposes no imports. Adding callbacks would
expand the capability surface and require separate reentrancy, lifetime, and
resource rules.

### Require a guest deallocator

The instance is never reused, so deallocation does not reclaim anything beyond
what dropping the store already reclaims. Cleanup would be another untrusted
call after success or failure.

### Reuse one mutable instance across messages

Persistent instances reduce instantiation cost but retain package-controlled
memory and globals between sessions or messages. Pooling, reset guarantees, and
tenant isolation need measurement and a focused lifecycle design.

### Adopt the WebAssembly component model immediately

WIT and canonical ABI tooling may later provide a better stable interface, but
that choice affects toolchains, artifact format, and compatibility policy. This
experiment proves the underlying bounded exchange needed by either approach.

## Out of scope

- serialization of framing or frame-processing values;
- stable export names, ABI versions, and compatibility negotiation;
- package SDK code generation and language support;
- semantic validation of transferred bytes;
- persistent instances, pooling, and reset protocols;
- guest deallocation for reused instances;
- host imports, WASI, and component-model enablement;
- accepted-message persistence and transport orchestration;
- production input and output limit values;
- compilation isolation, caching, signing, activation, and rollback.
