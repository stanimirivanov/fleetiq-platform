# Development guide

## TL;DR

- Use the pinned Rust toolchain and repository-owned commands.
- Run `cargo xtask repository` and `cargo xtask architecture` for immediate,
  database-independent structural feedback.
- Run the [dependency checks](dependency-management.md) before the
  database-backed Rust gate when reviewing a dependency or handing off a PR.
- Run PostgreSQL for SQLx-checked queries and the complete Rust acceptance
  gate.
- Use the [coding harness](harness.md) to select the applicable feedback tier
  and report unavailable checks honestly.

## Local setup

Start the complete local stack from the repository root:

```powershell
$env:FLEETIQ_POSTGRES_PORT = "55432"
$env:FLEETIQ_HTTP_PORT = "58080"
docker compose -f deploy/local/compose.yaml up --build --wait
Invoke-WebRequest http://127.0.0.1:$env:FLEETIQ_HTTP_PORT/health/ready
```

When running Rust processes on the host, start PostgreSQL by itself:

```powershell
docker compose -f deploy/local/compose.yaml up -d postgres
$env:DATABASE_URL = "postgres://fleetiq:fleetiq-local-only@localhost:$env:FLEETIQ_POSTGRES_PORT/fleetiq"
cargo install sqlx-cli --version 0.9.0 --locked --no-default-features --features rustls,postgres
sqlx migrate run --source migrations
$env:FLEETIQ_BIND_ADDRESS = "127.0.0.1:8080"
cargo run -p fleetiq-server
```

Rust 1.98.1, `rustfmt`, and Clippy are selected by `rust-toolchain.toml`.
PostgreSQL 18.6 is the supported development and CI version. The local database
is disposable; do not reuse its credentials outside local development.

Compose publishes PostgreSQL on `127.0.0.1:55432` and the backend on
`127.0.0.1:58080` by default. Choose other free host ports before starting the
project when required:

```powershell
$env:FLEETIQ_POSTGRES_PORT = "55433"
$env:FLEETIQ_HTTP_PORT = "58081"
```

Use the PostgreSQL value in host-side `DATABASE_URL`. Changing host ports does
not modify the database volume or container-side configuration.
`FLEETIQ_DATABASE_URL` may override the backend container's complete connection
string when a developer intentionally targets another database on the Compose
network.

The server requires `DATABASE_URL`, applies the embedded migration chain before
binding its listener, and defaults `FLEETIQ_BIND_ADDRESS` to
`127.0.0.1:8080`. Explicitly running migrations remains useful for SQLx
compilation and database inspection; applying them again at server startup is
safe and idempotent.

The protected asset query requires all three HTTP bearer identity variables
and a provisioned actor with a current tenant `metadata.read` grant. See the
[protected asset query procedure](../operations/asset-query.md) for its
configuration and HTTP contract. The default Compose stack does not configure
that credential, so it exposes only discovery and health until explicitly
configured.

The backend image builds with committed `.sqlx` query descriptions and
`SQLX_OFFLINE=true`; image builds therefore do not receive database credentials
or require access to a running database. After changing a checked query or
migration, regenerate and commit the metadata against the fully migrated
supported PostgreSQL version:

```powershell
cargo sqlx prepare --workspace -- --all-targets --all-features
```

## Quality gate

Start with the fast structural checks:

```powershell
cargo xtask repository
cargo xtask architecture
```

Then run the complete database-backed Rust gate from
[CONTRIBUTING.md](../../CONTRIBUTING.md#verification). Its commands include:

```powershell
cargo fmt --all --check
cargo sqlx prepare --check --workspace -- --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features --locked
cargo doc --workspace --no-deps --locked
```

SQLx macros connect to `DATABASE_URL` during compilation. Apply every migration
before running SQLx preparation, Clippy, tests, or documentation builds that
compile checked queries.

The repository and architecture commands need no database. The repository
command validates mechanical documentation and template policy. The
architecture command compares Cargo's workspace metadata with the reviewed
direct dependency policy.

Supply-chain checks use pinned CLI tools and no PostgreSQL. See the
[dependency management guide](dependency-management.md) for installation,
policy, and advisory-data requirements.

## Guides

- [Engineering standards](engineering-standards.md)
- [Coding harness and feedback tiers](harness.md)
- [Dependency management and supply-chain gate](dependency-management.md)
- [SQL migration criteria](sql-migrations.md)
- [Current implementation state](current-state.md)
- [Contributor workflow](../../CONTRIBUTING.md)
- [Architecture decisions](../adr/README.md)

Crates use `//!` module documentation as their overview. Add a nearby README
when a crate needs independent setup, runtime configuration, deployment,
publication, or an external package-author workflow.
