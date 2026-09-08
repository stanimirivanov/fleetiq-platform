# FleetIQ Platform

FleetIQ is an asset-centric telemetry and fleet-management platform. It models
vehicles, machines, infrastructure, sensors, and communication devices as typed
assets, without assuming a particular industry or identifier such as a VIN.

This repository is the production product monorepo. The earlier `fleetiq` and
`fleetiq-ui` repositories are historical proofs of concept and are not runtime
dependencies.

## Current scope

The first implementation step establishes the Rust workspace and the core domain
identifier types. Later increments will add the semantic asset model, protocol
runtime, telemetry processing, APIs, and web application described in
[`docs/architecture/README.md`](docs/architecture/README.md).

## Prerequisites

- Rust 1.98.1. The repository pins the toolchain and required components in
  `rust-toolchain.toml`.

## Build and verify

```shell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

The workspace currently contains no third-party Rust dependencies, so its domain
foundation can be built and tested without external services.

## Repository organization

```text
crates/domain/             Framework-independent business types and invariants
docs/architecture/         Accepted architecture boundaries
docs/adr/                  Architectural decisions and their consequences
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
