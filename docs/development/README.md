# Development guide

## TL;DR

Use the pinned Rust toolchain, run PostgreSQL for SQLx-checked queries and
integration tests, follow the workspace quality gate, and keep engineering and
migration rules in their dedicated guides.

## Local setup

From the repository root:

```powershell
docker compose -f deploy/local/compose.yaml up -d
$env:DATABASE_URL = "postgres://fleetiq:fleetiq-local-only@localhost:5432/fleetiq"
cargo install sqlx-cli --version 0.9.0 --locked --no-default-features --features rustls,postgres
sqlx migrate run --source migrations
```

Rust 1.98.1, `rustfmt`, and Clippy are selected by `rust-toolchain.toml`.
PostgreSQL 18.6 is the supported development and CI version. The local database
is disposable; do not reuse its credentials outside local development.

## Quality gate

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features --locked
cargo doc --workspace --no-deps --locked
```

SQLx macros connect to `DATABASE_URL` during compilation. Apply every migration
before running Clippy, tests, or documentation builds that compile checked
queries.

## Guides

- [Engineering standards](engineering-standards.md)
- [SQL migration criteria](sql-migrations.md)
- [Current implementation state](current-state.md)
- [Contributor workflow](../../CONTRIBUTING.md)
- [Architecture decisions](../adr/README.md)

Crates use `//!` module documentation as their overview. Add a nearby README
when a crate needs independent setup, runtime configuration, deployment,
publication, or an external package-author workflow.
