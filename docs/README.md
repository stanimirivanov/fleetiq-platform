# FleetIQ documentation

## TL;DR

Start with product scope and architecture. Use current state for delivered
behavior, milestones for intended sequence, development guides for engineering
rules, and ADRs for durable decisions. Rust API detail belongs in rustdoc.

## Product

- [Product scope](product/README.md) — users, vocabulary, goals, and non-goals.
- [Milestones](roadmap/milestones.md) — planned outcomes and sequencing.
- [Current state](development/current-state.md) — implemented behavior and
  deliberate limitations.

## Architecture

- [Architecture overview](architecture/README.md) — system context, boundaries,
  dependency direction, and data ownership.
- [Extensible ingestion](architecture/ingestion.md) — protocol packages,
  trusted-host gates, and durability stages.
- [Semantic asset graph](architecture/semantic-asset-graph.md) — assets,
  relationships, devices, bindings, and property attribution.
- [Telemetry processing](architecture/telemetry-processing.md) — decoded data,
  semantic changes, calculations, and live projections.
- [ADR index](adr/README.md) — accepted decisions and decision-record policy.

## Development

- [Contributor workflow](../CONTRIBUTING.md) — issues, pull requests,
  verification, and completion reports.
- [Engineering standards](development/engineering-standards.md) — Rust,
  architecture, testing, security, and operations.
- [SQL migrations](development/sql-migrations.md) — schema design,
  documentation, and verification criteria.
- [Security policy](../SECURITY.md) — private reporting and security-sensitive
  change expectations.

Crate-level API overviews live in each crate's `lib.rs` module documentation.
Add a crate README only when the crate gains independent setup, configuration,
operation, publication, or an external contributor workflow.
