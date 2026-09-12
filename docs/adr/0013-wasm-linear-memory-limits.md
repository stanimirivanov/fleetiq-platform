# ADR 0013: WebAssembly linear-memory limits

- Status: accepted
- Date: 2026-09-12

## Context

Fuel and epoch interruption bound the compute time of a protocol-package
invocation, but they do not bound guest memory. A package can declare a large
initial linear memory or repeatedly execute `memory.grow` while remaining within
its compute budget. Input buffers, package session state, and reply outputs are
already bounded outside the guest, but those limits do not constrain the
WebAssembly instance itself.

Wasmtime provides `ResourceLimiter` for store-owned WebAssembly resources. Its
documentation also makes clear that the limiter does not cover every allocation
made by a store or by host code. The runtime therefore needs a precise initial
linear-memory policy without presenting it as a complete process-memory limit.
See the Wasmtime
[`ResourceLimiter` documentation](https://docs.rs/wasmtime/48.0.2/wasmtime/trait.ResourceLimiter.html).

## Decision

Extend `InvocationBudget` with a nonzero maximum linear-memory size in bytes.
The budget now carries the three independent resources established by the
runtime experiment:

- deterministic fuel;
- approximate wall-clock time;
- guest linear-memory bytes.

Each invocation creates a host-owned `InvocationLimiter` and installs it on the
fresh Wasmtime store before instantiation. The limiter permits at most one
linear memory in that store. This makes the configured byte value an aggregate
guest linear-memory bound for the current probe rather than a per-memory value
that can be multiplied by declaring several memories.

The limiter checks every initial allocation and runtime growth request. A
request larger than the configured limit records the desired byte count and
returns an error, causing instantiation or guest execution to trap instead of
allowing WebAssembly's normal `memory.grow` failure value. The sandbox translates
that failure into `LinearMemoryLimitExceeded { limit, attempted }`. Other
allocation failures remain ordinary runtime errors.

WebAssembly linear memory grows in 64 KiB pages. A byte limit that is not page
aligned is still valid, but the largest usable memory is the greatest whole page
that fits. A module without linear memory can execute with a limit below one
page; a module declaring one page cannot.

The Wasmtime dependency keeps its threads feature disabled, so shared linear
memory is unavailable in this runtime. The engine also disables memory64
explicitly. These unsupported forms cannot bypass the synchronous limiter used
by the experiment.

Tests prove that one page is accepted at a 64 KiB limit, a two-page initial
memory is rejected, and growing from one page to two pages is rejected with the
same typed limit error.

## Consequences

- A protocol package cannot retain more guest linear memory than its invocation
  budget permits.
- Both module instantiation and runtime growth follow the same host policy.
- A typed error exposes the limit and attempted allocation for metrics and
  package diagnostics without depending on Wasmtime error text.
- Returning a limiter error makes policy violations trap immediately. Packages
  cannot catch a `memory.grow` result of `-1` and continue in a partially
  degraded state.
- Only one linear memory is currently supported. Supporting several would
  require aggregate accounting rather than applying the same maximum to each.
- The limit constrains logical WebAssembly linear-memory size. It does not bound
  compiler memory, Wasmtime metadata, native stacks, code pages, host-owned
  buffers, virtual address reservations, or total process resident memory.
- Store and process-level resource measurements remain necessary before setting
  production defaults.

## Alternatives considered

### Validate only the module's declared initial memory

This blocks oversized modules at activation but does not prevent an accepted
module from growing beyond the limit during message processing.

### Rely on a module-declared maximum

Customer-authored code controls that declaration and may omit it or choose a
value larger than the host's operational policy.

### Apply the same limit independently to multiple memories

The aggregate memory could become the configured limit multiplied by the number
of memories. Restricting the current runtime to one memory makes the budget's
meaning unambiguous.

### Let denied `memory.grow` return `-1`

That behavior follows normal WebAssembly semantics, but a protocol package could
continue after exceeding host policy and emit misleading partial output. A trap
gives the invocation one clear failure outcome.

### Treat the linear-memory limit as a complete memory sandbox

Wasmtime and host code allocate memory outside guest linear memory. Describing
this mechanism as a total memory limit would create an unsafe operational
assumption.

## Out of scope

- total process resident-memory enforcement;
- virtual-address reservation policy;
- compiler and compiled-code memory limits;
- native stack and Wasmtime metadata limits;
- host-owned input, state, and output limits already handled by other contracts;
- multiple or shared linear memories;
- memory64 packages;
- table element and component resource limits;
- process isolation and operating-system quotas;
- production memory defaults and tenant quotas;
- package activation and artifact validation workflows;
- the stable protocol package ABI.
