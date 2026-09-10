# FleetIQ Platform

FleetIQ is an asset-centric telemetry and fleet-management platform. It models
vehicles, machines, infrastructure, sensors, and communication devices as typed
assets, without assuming a particular industry or identifier such as a VIN.

This repository is the production product monorepo. The earlier `fleetiq` and
`fleetiq-ui` repositories are historical proofs of concept and are not runtime
dependencies.

## Current scope

The current implementation establishes the Rust workspace, semantic definition
catalogue, asset persistence, revisioned relationship persistence, and
communication-device endpoint persistence. Domain types cover tenants,
versioned definitions, assets, typed external identifiers, effective-dated asset
relationships, protocol-neutral device endpoints, and revisioned temporal signal
bindings. Later increments will add signal-binding persistence, conversion and
source-selection policies, the protocol runtime, telemetry processing, APIs, and
the web application described in
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
crates/application/        Use cases and application-owned infrastructure ports
crates/postgres/           Private SQLx row mapping and PostgreSQL adapters
migrations/                Authoritative PostgreSQL metadata schema
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
