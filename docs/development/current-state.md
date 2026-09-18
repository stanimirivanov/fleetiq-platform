# Current implementation state

## TL;DR

- The repository includes the first runnable backend composition root and a
  provider-neutral tenant authorization boundary, but no product or
  administrative API.
- Asset definitions, assets, relationships, devices, signal bindings, accepted
  messages, decoded results, and protocol releases have PostgreSQL contracts.
- Protocol framing and processing are bounded and a versioned WIT component
  boundary is admitted and invoked through Wasmtime.
- Semantic resolution, calculator output, and live projection contracts exist;
  their production workers and stores do not.
- The complete development stack runs through Compose on isolated loopback
  ports; production deployment remains a later milestone.
- Business APIs, identity-provider integration, authorization persistence,
  device transport listeners, operator UI, archival storage, and observability
  remain future milestones.

## Semantic metadata foundation

The domain and PostgreSQL adapters support:

- tenant-scoped immutable versions of asset types, property definitions, and
  relationship types;
- assets with internal identity, a pinned asset-type version, and typed external
  identifiers;
- revisioned, effective-dated relationships between tenant-owned assets;
- communication devices and ordered extensible endpoints; and
- revisioned, effective-dated signal bindings from device sources to exact
  asset property versions.

Repository operations enforce tenant-safe references, append order, bitemporal
selection, and validated row-to-domain reconstruction. The current model does
not prescribe locomotive, road-vehicle, refrigeration, or other industry
schemas.

## Protocol contracts and host policy

The protocol SDK and trusted host implement:

- bounded incremental framing over fragmented and coalesced input;
- bounded opaque connection state with atomic whole-state replacement;
- proposed replies with processing or durability release conditions;
- a two-stage framing and complete-frame processing contract;
- atomic validation of output counts, reply counts and bytes, and state bytes;
- ownership-preserving failure values for retry; and
- a bounded session turn that extracts and processes at most one frame.

The host has no production network listener or device-session registry yet.

## WebAssembly protocol components

The Wasmtime integration implements:

- raw artifact limits before compilation;
- rejection of undeclared host imports and unsupported table shapes;
- deterministic fuel and external wall-clock interruption;
- explicit approximate guest stack, linear-memory, and table limits;
- validated pointer-length transfer for the earlier core-module experiment;
- a versioned WIT world for framing and processing;
- exact-world admission with typed pre-instantiation bindings;
- host validation of lifted values; and
- fresh bounded stores and component instances for each invocation.

Admitted components retain their immutable package release and host-computed
SHA-256 artifact digest. PostgreSQL catalogue registration is implemented
separately from admission, including exact retry, immutable-content collision,
tenant-scoped lookup, and the complete unsigned artifact-size range. Activation
policy, artifact storage, compilation isolation, package distribution, and
author tooling remain planned.

## Durable ingestion

The application and PostgreSQL adapters implement two append-only boundaries:

1. accepted messages retain tenant, device, package release, receipt time, and
   exact wire bytes; and
2. decoded results retain one completion marker and the complete ordered set of
   typed observations for that accepted message.

Both distinguish first append, exact retry, and immutable identity collision.
Decoded marker and child rows append atomically, including zero-output results.
Application orchestration withholds decoded values and durability-gated replies
until their required append succeeds and returns exact pending work on failure.
A PostgreSQL recovery harness covers suppressed successful responses and exact
retry across both durability stages.

## Semantic and live contracts

The semantic resolver applies bitemporal bindings and exact property
definitions, returning explicit unresolved outcomes for missing or incompatible
configuration. Successful binding produces asset property change candidates
with event-time basis and deterministic interpretation identity.

Analytics contracts represent versioned calculator evaluations and typed
semantic outputs. Live contracts represent full snapshots, forward-only deltas,
cursor continuity, and resnapshot reasons. No production semantic worker,
property history store, calculator runtime, subscription transport, or operator
dashboard exists yet.

## Runtime and data

The workspace is Rust 2024 on Rust 1.98.1. PostgreSQL 18.6 stores metadata and
ingestion facts under checked SQL migrations. SQLx macros validate queries
against a migrated database. Committed offline query descriptions permit
database-independent image builds while online checks remain authoritative.

The `fleetiq-server` binary validates environment configuration, applies the
embedded migration chain, exposes liveness and PostgreSQL-backed readiness
probes, and shuts down gracefully after `Ctrl+C`. It is an operable development
composition root, not a production deployment. Unix deployments also handle
`SIGTERM`, which is the normal container stop signal.

The local Compose deployment builds that binary in a multi-stage image, waits
for PostgreSQL readiness, publishes both services only on loopback, and runs the
backend as a non-root user with a read-only filesystem and dropped Linux
capabilities. It is disposable development infrastructure rather than a
production manifest.

## Identity and tenant access

The domain defines a stable internal actor identity that does not encode an
identity provider. The application authorization service evaluates an explicit
actor, tenant, and action tuple through a consumer-owned policy port. Only an
allow decision produces typed, exact-operation authorization evidence; missing
grants and policy failures fail closed. Tests cover tenant and action isolation
and distinguish ordinary denial from policy unavailability.

No actor store, external-identity mapping, membership or role model, policy
adapter, authentication middleware, protected route, or authorization audit log
exists yet. The boundary defines how those later slices must enter protected
application behavior without treating a caller-supplied tenant identifier as a
grant.

There is no business API, authentication provider, authorization policy
adapter, message broker, object archive, cache, production deployment manifest,
or service-level objective. Do not describe library contracts or local
database defaults as production behavior.

## Deliberate limitations

- No Teltonika or other real device package is distributed.
- No package activation, connection routing, network transport, or command path.
- No semantic history persistence or end-to-end ingestion worker.
- No tenant/user administration, authentication, authorization, or audit UI.
- No live map, graph editor, dashboard, alert delivery, or query API.
- No archive, retention enforcement, backup/restore, or cost controls.
- No hardware qualification fixtures or field-device interoperability suite.
- No production observability, scaling, rollout, or recovery automation.

The intended sequence is maintained in
[implementation milestones](../roadmap/milestones.md). Update this document when
a merged capability materially changes what the repository can do.
