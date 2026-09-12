# ADR 0015: WebAssembly table resource limits

- Status: accepted
- Date: 2026-09-12

## Context

The protocol-package runtime limits fuel, elapsed execution, and guest linear
memory for every invocation. Wasmtime tables are a separate guest resource.
They hold references used by indirect calls and other WebAssembly features, and
they can declare a large initial element count or grow while a module runs.

The existing resource limiter allowed every table allocation and inherited
Wasmtime's default count limit of 10,000 tables per store. A package could
therefore consume host memory through tables without crossing its linear-memory
ceiling. Wasmtime currently represents each table element with pointer-sized
storage, although the host policy should be expressed in logical elements
rather than relying on that implementation detail. See the Wasmtime
[`ResourceLimiter` documentation](https://docs.rs/wasmtime/48.0.2/wasmtime/trait.ResourceLimiter.html).

The host needs one clear aggregate table policy for the temporary probe runtime
without deciding the eventual protocol ABI or which compiler-generated table
layouts production packages will use.

## Decision

Extend `InvocationBudget` with a nonzero maximum table-element count. The limit
is installed in the same fresh store-owned `InvocationLimiter` as the fuel,
deadline, and linear-memory policy.

The limiter checks initial table allocation and runtime growth. A requested
element count greater than the invocation limit records the attempted count and
returns an error. This traps instantiation or execution instead of allowing
`table.grow` to return its normal failure value. The sandbox maps the recorded
denial to `TableElementLimitExceeded { limit, attempted }`; unrelated Wasmtime
failures remain ordinary runtime errors.

The probe runtime supports at most one table per module. With one table, the
configured element value is both a per-table and aggregate guest table limit.
`WasmSandbox::compile` inspects the successfully compiled module's required
resources and returns `TooManyTables { limit, actual }` during admission if the
module declares more than one table. The store limiter independently retains a
one-table ceiling as defense in depth.

A module may declare no table. A table exactly at the configured element limit
is accepted, while initial allocation or growth beyond it traps with the same
typed policy error.

## Consequences

- A package cannot allocate or grow its one guest table beyond the invocation's
  explicit element allowance.
- Initial table allocation and `table.grow` follow the same host policy.
- The table limit cannot be multiplied by declaring several tables.
- Modules with more than one table fail during admission rather than after
  activation on a device-message path.
- Limit errors carry logical element counts suitable for diagnostics and
  metrics without parsing Wasmtime error text.
- Returning a limiter error stops the invocation immediately. A package cannot
  continue after a denied `table.grow` and produce partial output.
- The element limit constrains Wasmtime table storage, not all memory used by
  the instance, runtime, compiler, or host.
- The one-table restriction may need revision after the stable package toolchain
  and ABI produce representative modules.

## Alternatives considered

### Rely on the linear-memory limit

WebAssembly tables are allocated separately from guest linear memory. Their
elements do not count against the linear-memory byte ceiling.

### Keep Wasmtime's default table-count limit

The default permits many tables and does not restrict the number of elements in
each table. It is too broad for an untrusted per-message runtime policy.

### Apply the element limit independently to multiple tables

The aggregate allowance would become the configured limit multiplied by the
number of tables. Restricting the probe runtime to one table gives the budget an
unambiguous meaning.

### Let denied `table.grow` return its normal failure value

A package could continue after exceeding host policy and return incomplete or
misleading output. Trapping gives the host one deterministic failure outcome.

### Reject every module containing a table

Compiler-generated WebAssembly can use a table for indirect function calls.
Allowing one bounded table preserves that capability without granting unbounded
storage.

## Out of scope

- stable package exports, imports, serialization, and ABI;
- production table-element defaults and tenant-specific budgets;
- multiple-table support and aggregate accounting across several tables;
- compiler, runtime metadata, native stack, and total process-memory limits;
- component-model resources, garbage-collected references, and WASI;
- pooling allocator selection and store reuse;
- package activation, caching, signatures, and distribution;
- protocol-specific package implementations.
