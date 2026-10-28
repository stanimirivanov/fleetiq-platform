# FleetIQ documentation map

## TL;DR

- Start with [CONTRIBUTING.md](../CONTRIBUTING.md), then use this map to load
  only the sources relevant to the task.
- Product intent, current implementation, architecture, engineering rules, and
  durable decisions have separate canonical homes.
- Update this map when a canonical source moves or a recurring task lacks a
  clear route.
- Rust API detail belongs in rustdoc; add a crate README only when that crate
  has an independent contributor or operator workflow.

## Purpose and authority

This page is the progressive-disclosure entry point for contributors and
coding agents. It routes readers to canonical sources rather than repeating
their policy. [CONTRIBUTING.md](../CONTRIBUTING.md) owns workflow, issue shape,
verification, and completion reporting. [AGENTS.md](../AGENTS.md) is the
concise tool-facing entry point.

Read the rows that match the work. Do not bulk-read every guide or ADR. When a
change affects product behavior or a dependency boundary, always consult the
product scope and relevant architecture page.

## Route by task

| Task or changed area | Read before changing it | Verify or consult as needed |
|:--|:--|:--|
| Product vocabulary, supported outcomes, or non-goals | [Product scope](product/README.md) | [Current state](development/current-state.md), relevant [ADRs](adr/README.md) |
| Capability ownership, dependency direction, or deployment boundaries | [Architecture overview](architecture/README.md), [dependency rules](architecture/dependency-rules.md) | [Engineering standards](development/engineering-standards.md), `cargo xtask architecture` |
| Rust implementation, API design, or tests | [Engineering standards](development/engineering-standards.md) | Relevant capability page, `cargo xtask repository`, complete Rust gate |
| PostgreSQL schema, SQLx query, migration, or persisted meaning | [SQL migration criteria](development/sql-migrations.md), relevant persistence ADR | `cargo xtask migrations --base origin/main`, `.sqlx` metadata, database-backed quality gate |
| Protocol packages, framing, sessions, or Wasm execution | [Extensible ingestion](architecture/ingestion.md), [guest authoring](../examples/custom-protocol/README.md), [Teltonika support](../packages/teltonika/README.md), [artifact admission](operations/protocol-package-admission.md), [route commissioning](operations/protocol-route-commissioning.md), [diagnostics and measured load](operations/ingestion-diagnostics.md) | Relevant ingestion ADRs and host-boundary tests |
| Asset graph, signal binding, or property attribution | [Semantic asset graph](architecture/semantic-asset-graph.md) | [Telemetry processing](architecture/telemetry-processing.md), [durable semantic worker](operations/semantic-worker.md), relevant ADRs |
| Calculators, projections, or live telemetry behavior | [Telemetry processing](architecture/telemetry-processing.md) | [Semantic asset graph](architecture/semantic-asset-graph.md), relevant ADRs |
| Server composition, identity, authorization, or HTTP API | [Architecture overview](architecture/README.md), [current state](development/current-state.md) | [Security policy](../SECURITY.md), [browser identity](operations/browser-identity.md), relevant ADRs |
| HTTP response contract, client generation, or fixtures | [Backend-owned HTTP contract](../contracts/http/README.md), [OpenAPI maintenance skill](../.agents/skills/openapi/SKILL.md), [HTTP versioning ADR](adr/0048-establish-versioned-http-api-boundary.md) | Server contract tests and relevant protected-route tests |
| Bootstrap, deployment, readiness, or local runtime behavior | [Operations index](operations/README.md), [development guide](development/README.md) | [Backend image publication](operations/image-publication.md), [Runtime and migrations](operations/runtime-and-migrations.md), [Platform bootstrap](operations/platform-bootstrap.md), Compose smoke checks |
| Recurring repository sensor or modular HTTP contract work | [xtask scoped agreement](../crates/xtask/AGENTS.md), [HTTP scoped agreement](../contracts/http/AGENTS.md); load only the owning scope | [Scoped guidance policy](development/harness.md#scoped-guidance), repository and relevant sensor fixtures |
| Build, CI, repository policy, agent guidance, or developer experience | [Coding harness](development/harness.md), [harness protection](development/harness-protection.md), [verification inventory and evidence](development/verification.md), [development guide](development/README.md) | Structural, history, secret, bypass, and reviewed CI wiring checks; complete applicable gate |
| Dependency additions, advisories, licenses, sources, or update proposals | [Dependency rules](architecture/dependency-rules.md), [dependency management](development/dependency-management.md) | `cargo xtask architecture`, `cargo deny check`, `cargo machete --with-metadata --skip-target-dir` |
| Milestone or issue planning | [Milestones](roadmap/milestones.md), [CONTRIBUTING.md](../CONTRIBUTING.md) | Relevant product and architecture sources |
| Vulnerability reporting or security-sensitive behavior | [Security policy](../SECURITY.md) | Relevant architecture and ADRs |

## Canonical collections

- [Product](product/README.md) defines users, vocabulary, goals, and non-goals.
- [Architecture](architecture/README.md) describes system boundaries,
  information flow, trust, and direct dependency policy.
- [HTTP contracts](../contracts/http/README.md) contain backend-owned client artifacts.
- [Development](development/README.md) contains setup, engineering, migration,
  current-state, and harness guidance.
- [Operations](operations/README.md) contains installation and runtime
  procedures.
- [ADRs](adr/README.md) record durable choices and their lifecycle.
- [Roadmap](roadmap/milestones.md) groups intended delivery into outcome
  milestones.
- [Security](../SECURITY.md) defines vulnerability reporting and expectations
  for security-sensitive changes. [CODE_OF_CONDUCT.md](../CODE_OF_CONDUCT.md)
  defines community behavior.

## Keeping the map useful

A change must update this page when it adds, moves, renames, or removes a
canonical source or creates a recurring task without a route. Avoid listing
short-lived implementation details. `cargo xtask repository` checks mechanical
documentation policy and repository-local links; semantic accuracy and stale
guidance still require review.

For bounded asset/property history and immutable evidence, use the
[property-history operations guide](operations/property-history.md) and
[ADR 0119](adr/0119-expose-bounded-property-history.md).

For bounded metadata exploration, use the
[semantic graph operations guide](operations/semantic-graph.md) and
[ADR 0120](adr/0120-explore-bounded-semantic-graphs.md).

For durable current-property materialization, authorized snapshots and atomic replay,
use the [current-state operations guide](operations/current-property-state.md) and
[ADR 0121](adr/0121-materialize-durable-current-property-state.md).

For demand-driven current-property SSE, scoped resume and recovery, use the
[live operations guide](operations/property-live-updates.md) and
[ADR 0122](adr/0122-deliver-authorized-property-state-over-sse.md).
