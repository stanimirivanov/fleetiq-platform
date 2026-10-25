# ADR 0112: Store artifacts and isolate protocol admission

## Status

Proposed, 25 October 2026.

Supersedes: none. Superseded by: none.

## Context

Immutable release metadata does not retain executable content or prove that
its bytes meet the host WIT/import policy. Untrusted Wasmtime compilation has
no invocation fuel budget and must not run on HTTP runtime threads. General
metadata-management authority must not implicitly authorize code admission.

## Decision

Use the existing tenant release catalogue, PostgreSQL-owned bounded artifact
bytes and an ingestion-owned compiler port. Introduce explicit `protocol.manage`
authority; existing and newly provisioned tenants receive no automatic grant.
Tenant access administrators delegate it through the existing audited grant API.

Registration checks the expected SHA-256 against host-hashed bytes before any
write. A catalogue record may commit before artifact storage fails; the same
release/content retry completes storage. Reads report a stored release only
when exact bytes are present and verified. Different content never overwrites
an existing release. No externally visible success represents partial storage.

Admission selects `runtime-v1` or the optional `identity-v1` explicitly. It checks
stored digest/length again and records successful evidence keyed by release,
world and reviewed policy version. Concurrent retries retain first actor/time.
Failed compilation creates no successful receipt. Admission is not activation.

A separate sibling executable receives only bounded bytes and the fixed world
name. It inherits no application environment, uses no shell, emits no captured
compiler output, never invokes guest exports and rejects host imports through
the maintained Wasmtime boundary. One compiler slot per backend process has no
unbounded queue. A supervisor retains that slot until cancellation/timeout
kills and reaps the child. Linux applies hard 2 GiB virtual-address and 10 CPU
second limits before parsing; the parent also enforces a 15-second deadline.
Core dumps are disabled. Unsupported platforms or missing workers fail closed.

Reuse `rlimit` 0.11.0's safe maintained resource APIs instead of handwritten
unsafe syscall glue. It is Linux-targeted in the Wasm adapter; it adds no
dependency to domain, access or the HTTP process. SHA-256 uses the existing
workspace sha2 dependency in the pure ingestion value boundary. No new service,
object store, native-code cache or compiled artifact format is introduced.

## Consequences

An initial 8 MiB raw ceiling bounds uploads and database values. PostgreSQL
storage provides atomic visibility and shared backup/restore, with storage cost
that must be revisited using measurements. Retention/GC is deliberately absent;
immutable database triggers protect bytes and successful evidence.

Process limits bound one compiler, not aggregate replicas or total host memory.
Deployments still need container memory/CPU/PID limits and admission-rate policy.
This contains compiler crashes/resource exhaustion; it is not a kernel security
sandbox against a native compiler exploit. Guest host imports remain forbidden.

Changing WIT semantics, compiler isolation policy or bounds requires a new
reviewed policy version and fresh evidence. Native compiled code is never
persisted/deserialized. Activation and device routing remain subsequent work.

Raw artifacts are addressed by `(tenant, SHA-256)`; identical releases share
bytes only within one tenant. Admission foreign keys bind both the exact
release and its digest, independently of the selected world/policy key.

HTTP baseline.13 expands the closed action response vocabulary. Existing grants
remain unchanged; operators must upgrade clients with closed action enums before
explicitly delegating protocol.manage. The new command routes remain under v1.

## Alternatives considered

- Compile within the HTTP process: rejected because a compiler crash or memory
  exhaustion could take down requests and bypass invocation fuel limits.
- Store uploads in an external object store: deferred until artifact volume
  justifies another credential, consistency and backup boundary. Tenant-local
  content-addressed PostgreSQL blobs reuse the existing durable store.
- Persist compiled native artifacts: rejected because their unsafe loading and
  compiler/version compatibility create a different security boundary.
- Handwritten resource-limit syscalls: rejected in favor of the maintained safe
  `rlimit` API; unsupported platforms refuse admission.
