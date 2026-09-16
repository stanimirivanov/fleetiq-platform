# Current implementation state

## TL;DR

- The repository implements a Rust domain and persistence foundation, not a
  deployable FleetIQ product.
- Asset definitions, assets, relationships, devices, signal bindings, accepted
  messages, decoded results, and protocol releases have PostgreSQL contracts.
- Protocol framing and processing are bounded and a versioned WIT component
  boundary is admitted and invoked through Wasmtime.
- Semantic resolution, calculator output, and live projection contracts exist;
  their production workers and stores do not.
- APIs, authentication, transport listeners, operator UI, archival storage,
  observability, and deployment remain future milestones.

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
SHA-256 artifact digest. Catalogue registration is implemented separately from
admission. Activation policy, artifact storage, compilation isolation, package
distribution, and author tooling remain planned.

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
against a migrated database. The local Compose deployment runs only PostgreSQL.

There is no production binary, public API, authentication provider, message
broker, object archive, cache, deployment manifest, or service-level objective.
Do not describe library contracts or local database defaults as production
behavior.

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
