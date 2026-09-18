# FleetIQ Platform

FleetIQ is an asset-centric telemetry and fleet-management platform. It models
vehicles, machines, infrastructure, sensors, and communication devices without
assuming an industry, hardware vendor, or universal identifier such as a VIN.

## TL;DR

- This repository contains the production-oriented FleetIQ backend foundation.
- Rust domain and application crates keep protocol, persistence, and runtime
  details behind explicit boundaries.
- PostgreSQL is the source of truth for metadata and durable ingestion facts.
- Untrusted, extensible protocol packages decode device traffic under
  host-controlled limits.
- The project is pre-release; the implemented capability inventory lives in
  [current state](docs/development/current-state.md).

## System flow

```mermaid
flowchart LR
    Device[Telemetry device] --> Transport[Transport host]
    Transport --> Runtime[Bounded protocol runtime]
    Runtime --> Raw[(Accepted messages)]
    Raw --> Decoded[(Decoded observations)]
    Decoded --> Semantics[Semantic binding]
    Semantics --> Twin[Asset property history]
    Twin --> Analytics[Calculators and alerts]
    Twin --> API[Application APIs]
    API --> UI[Operator experience]
```

Protocol packages describe vendor data. Signal bindings connect decoded
signals to properties of tenant-owned assets. This separation lets the same
core represent a truck and its refrigeration unit, a locomotive and its
traction equipment, or another customer-defined asset graph.

## Quickstart

Requirements:

- Rust 1.98.1, pinned by [`rust-toolchain.toml`](rust-toolchain.toml);
- Docker with Compose.

Start the local database and configure checked SQL queries:

```powershell
$env:FLEETIQ_POSTGRES_PORT = "55432"
docker compose -f deploy/local/compose.yaml up -d
$env:DATABASE_URL = "postgres://fleetiq:fleetiq-local-only@localhost:$env:FLEETIQ_POSTGRES_PORT/fleetiq"
cargo install sqlx-cli --version 0.9.0 --locked --no-default-features --features rustls,postgres
sqlx migrate run --source migrations
```

Run the repository quality gate:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features --locked
cargo doc --workspace --no-deps --locked
```

Port `55432` is the FleetIQ local default. Set `FLEETIQ_POSTGRES_PORT` to another
free host port before starting Compose when necessary. The container continues
to use port `5432`. The checked-in database password is for disposable local
development only.

## Repository map

| Path | Responsibility |
|:--|:--|
| [`crates/domain`](crates/domain) | Framework-independent asset and semantic metadata invariants |
| [`crates/telemetry`](crates/telemetry) | Protocol-neutral accepted-message and observation contracts |
| [`crates/protocol-sdk`](crates/protocol-sdk) | Types shared by protocol packages and the trusted host |
| [`crates/protocol-host`](crates/protocol-host) | Host-owned framing, state, reply, and processing policies |
| [`crates/protocol-wasm`](crates/protocol-wasm) | Bounded Wasmtime component admission and invocation |
| [`crates/semantics`](crates/semantics) | Temporal signal-to-asset-property attribution |
| [`crates/analytics`](crates/analytics) | Calculator identities and output contracts |
| [`crates/application`](crates/application) | Use cases and consumer-owned infrastructure ports |
| [`crates/postgres`](crates/postgres) | SQLx persistence adapters and private row mappings |
| [`migrations`](migrations) | Authoritative PostgreSQL schema history |
| [`deploy/local`](deploy/local) | Disposable local infrastructure |
| [`docs`](docs/README.md) | Product, architecture, development, roadmap, and decisions |

Folders are introduced with working behavior. Deployable applications and new
crates arrive with the first capability that requires them.

## Start here

- [Product scope](docs/product/README.md)
- [Architecture](docs/architecture/README.md)
- [Current implementation state](docs/development/current-state.md)
- [Implementation milestones](docs/roadmap/milestones.md)
- [Contributor workflow](CONTRIBUTING.md)
- [Engineering standards](docs/development/engineering-standards.md)
- [Architecture decisions](docs/adr/README.md)
- [Security policy](SECURITY.md)
- [Code of conduct](CODE_OF_CONDUCT.md)

The earlier `fleetiq` and `fleetiq-ui` repositories are historical proofs of
concept and are not runtime dependencies.
