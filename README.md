# FleetIQ Platform

FleetIQ is an asset-centric telemetry and fleet-management platform. It models
vehicles, machines, infrastructure, sensors, and communication devices as typed
assets, without assuming a particular industry or identifier such as a VIN.

This repository is the production product monorepo. The earlier `fleetiq` and
`fleetiq-ui` repositories are historical proofs of concept and are not runtime
dependencies.

## Current scope

The current implementation establishes the Rust workspace, semantic definition
catalogue, asset persistence, revisioned relationship persistence,
communication-device endpoint persistence, and revisioned signal-binding
persistence. The protocol-neutral telemetry contract represents typed decoded
observations with deterministic replay identity, source metadata, separate
device and receipt times, quality, units, and decoder provenance. It also
represents durably accepted protocol frames with admission identity, pinned
package provenance, receipt time, and opaque wire bytes. A protocol SDK gives
packages a constrained decoded-output type and lets the trusted host add
admission and replay provenance. A protocol-host framing experiment retains
fragmented input, separates coalesced frames, and enforces a hard unread-byte
limit while packages inspect only borrowed bytes. The host also retains bounded,
opaque, connection-scoped package state and applies whole-state replacements
atomically. Packages propose size-bounded replies, and the host withholds
data-acceptance replies until it receives durable-acceptance evidence. The
protocol SDK composes these capabilities into a two-stage semantic invocation:
side-effect-free framing followed by complete-frame processing that returns one
ordered proposal for decoded outputs, session state, and replies. The protocol
host validates this proposal atomically with explicit output-count, reply-count,
individual-reply, aggregate-reply, and session-state limits. An isolated
Wasmtime experiment proves deterministic fuel exhaustion and external
wall-clock interruption without defining the final package ABI. The sandbox has
an explicit approximate guest stack ceiling, and its per-invocation resource
policy bounds one guest linear memory and one guest table during initial
allocation and runtime growth. Raw WebAssembly modules are size-bounded before
compilation, and modules requesting unavailable host imports or multiple tables
fail admission. The host-selected temporary probe must be a no-argument,
no-result function and is bound to the compiled module during admission. A
separate transfer experiment binds guest memory, allocation, and processing
exports and exchanges independently bounded opaque bytes through validated
pointer-length ranges in a fresh instance. The first versioned WIT world defines
language-neutral framing and processing operations over the WebAssembly
Component Model and is compile-checked through generated Wasmtime bindings. A
host adapter copies semantic inputs into component-owned values and validates
lifted frame decisions and processing results through the existing SDK domain
constructors without retaining rejected package values in errors. Component
admission applies a raw-byte ceiling before compilation, rejects host imports,
checks the exact embedded WIT world, and retains typed pre-instantiation
bindings. Each typed framing or processing call uses a fresh bounded store and
component instance, separates package rejection from runtime failure, and returns
only output validated through the semantic SDK. Complete-frame processing then
applies that output through the trusted host gate using the same session-state
object supplied to the component, preserving prior state when invocation or
host validation fails. Component framing likewise operates on the bounded host
input buffer, extracts at most one validated frame, and preserves unread bytes
on incomplete input or failure. A bounded session-turn operation composes these
stages for at most one frame, keeps their invocation budgets and reports
separate, and returns an extracted frame to the caller if its subsequent
processing fails. The application layer defines an append-only accepted-message
port that turns complete pending frames into durable facts, supports exact
idempotent retries, and rejects identity reuse with different immutable content.
PostgreSQL provides the tenant-scoped, append-only accepted-message schema that
retains exact wire bytes and trusted device, package, and receipt-time
provenance for that durability boundary. Its application-port adapter performs
first append in one statement and uses a fresh-snapshot primary-key read only
after conflict, returning an exact retry separately from an immutable-content
identity collision.

The semantic resolver applies bitemporal bindings and exact property definitions
while retaining unconfigured or incompatible observations for diagnosis and
retry. Successfully bound values become
asset-property change candidates with explicit event-time selection and
deterministic semantic-interpretation identity. Versioned calculator evaluations
produce validated semantic property outputs with separate event and evaluation
times and deterministic replay identity. Transport-independent live contracts
define complete projection snapshots, forward-only deltas, cursor continuity,
and explicit resnapshot recovery.
Domain types cover tenants, versioned definitions, assets, typed external
identifiers, effective-dated asset relationships, protocol-neutral device
endpoints, and revisioned temporal signal bindings. Later increments will add
event serialization, conversion and source-selection policies, the protocol
runtime, persistence, APIs, and the web application described in
[`docs/architecture/README.md`](docs/architecture/README.md).

## Prerequisites

- Rust 1.98.1. The repository pins the toolchain and required components in
  `rust-toolchain.toml`.
- Docker with Compose for PostgreSQL-backed development and integration tests.

## Build and verify

```shell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

Checked SQL compilation and PostgreSQL integration tests require a migrated
database:

```powershell
docker compose -f deploy/local/compose.yaml up -d
$env:DATABASE_URL = "postgres://fleetiq:fleetiq-local-only@localhost:5432/fleetiq"
cargo install sqlx-cli --version 0.9.0 --locked --no-default-features --features rustls,postgres
sqlx migrate run --source migrations
cargo test --workspace --all-features
```

The password is explicitly local-only. Use environment secret management for
deployed databases.

## Repository organization

```text
crates/domain/             Framework-independent business types and invariants
crates/analytics/          Calculator output contracts and analytics policies
crates/application/        Use cases and application-owned infrastructure ports
crates/postgres/           Private SQLx row mapping and PostgreSQL adapters
crates/protocol-host/      Host-owned protocol framing and runtime policies
crates/protocol-sdk/       Protocol-package output and host assembly contracts
crates/protocol-wasm/      Isolated Wasmtime runtime feasibility experiments
crates/semantics/          Temporal attribution and normalization policies
crates/telemetry/          Protocol-neutral decoded observation contracts
migrations/                Authoritative PostgreSQL product schemas
deploy/local/              Disposable local development infrastructure
docs/architecture/         Accepted architecture boundaries
docs/adr/                  Architectural decisions and their consequences
docs/development/          Contributor standards and implementation guidance
```

Folders are introduced with working code. Deployable applications and additional
crates will be added by the pull request that first needs them.

## Design rules

- Domain code does not depend on transports, databases, brokers, or runtime
  frameworks.
- Tenant ownership is explicit at application and persistence boundaries.
- Assets use FleetIQ internal identities and typed external identifiers; no
  industry-specific identifier is universal.
- Protocol decoding and semantic attribution remain separate concerns.
- Public types and behavior are documented. Tests focus on domain invariants and
  externally observable behavior.

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the development workflow.
