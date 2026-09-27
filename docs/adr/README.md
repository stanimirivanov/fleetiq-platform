# Architecture decision records

## TL;DR

Use an ADR for durable choices that affect contracts, persisted meaning,
security, deployment topology, foundational technology, or several crates.
Accepted ADRs are historical records: supersede them instead of rewriting their
decision. The architecture pages describe the current system.

## Lifecycle

1. Copy [`0000-template.md`](0000-template.md) to the next zero-padded number
   and a short kebab-case title.
2. Use status `Proposed` while the decision is under review.
3. Change it to `Accepted` when the change is approved.
4. Use `Superseded by ADR NNNN` when a later decision replaces it, and link
   both records.
5. Use `Rejected` for a proposal retained to prevent repeated investigation.

Do not renumber records. Correct spelling or broken links in an accepted ADR,
but do not rewrite its context, decision, alternatives, or consequences to make
history match the current system.

## Required content

Every record states status, context, decision, consequences, and alternatives.
Include compatibility, migration, security, operational, and rollback effects
when relevant. Link the code, schema, or contract whose meaning the ADR governs.

## Index

| Range | Decisions |
|:--|:--|
| [0001](0001-postgresql-sqlx-persistence.md) | PostgreSQL and SQLx persistence |
| [0002–0008](0002-decoded-observation-contract.md) | Telemetry, semantic resolution, accepted messages, property changes, analytics, and live projection |
| [0009–0020](0009-bounded-incremental-protocol-framing.md) | Host framing, session state, replies, Wasmtime limits, invocation, and transfer |
| [0021](0021-lock-revision-head-before-dependent-read.md) | PostgreSQL revision locking and fresh-snapshot reads |
| [0022–0028](0022-versioned-protocol-package-component-contract.md) | Versioned WIT component admission and bounded session invocation |
| [0029–0037](0029-append-accepted-messages-idempotently.md) | Idempotent raw and decoded ingestion durability and recovery |
| [0038–0040](0038-bind-admitted-components-to-release-and-artifact.md) | Protocol artifact identity and immutable release catalogue |
| [0041](0041-establish-single-backend-composition-root.md) | Backend composition root, runtime, and health surface |
| [0042](0042-establish-per-action-tenant-authorization-boundary.md) | Provider-neutral actor identity and per-action tenant authorization |
| [0043](0043-persist-exact-tenant-action-grants.md) | PostgreSQL actors, exact tenant action grants, and policy evaluation |
| [0044](0044-administer-tenant-grants-with-atomic-audit.md) | Authorized idempotent grant administration with atomic immutable audit |
| [0045](0045-handle-all-accepted-message-unique-arbiters.md) | Accepted-message idempotency across every unique arbiter |
| [0046](0046-map-external-identities-to-internal-actors.md) | Exact external identity mapping to stable internal actors |
| [0047](0047-require-authenticated-actor-evidence-for-authorization.md) | Authenticated actor evidence required by tenant authorization |
| [0048](0048-establish-versioned-http-api-boundary.md) | Versioned HTTP discovery and bounded problem details |
| [0049](0049-separate-platform-from-tenant-authorization.md) | Exact platform authorization distinct from tenant access |
| [0050](0050-provision-tenants-atomically-with-initial-access.md) | Atomic tenant, initial access, and immutable provisioning audit |
| [0051](0051-bootstrap-the-first-platform-operator-once.md) | One-time atomic establishment of the first platform operator |
| [0052](0052-invoke-bootstrap-through-an-explicit-process-command.md) | Explicit one-shot process command for controlled platform bootstrap |
| [0053](0053-provision-actors-atomically-with-initial-identity.md) | Authorized atomic actor provisioning with an initial external identity |
| [0054](0054-administer-platform-grants-with-atomic-audit.md) | Authorized idempotent platform grant administration with atomic immutable audit |
| [0055](0055-query-platform-access-audit-with-keyset-pages.md) | Authorized bounded platform access audit queries with stable keyset pagination |
| [0056](0056-administer-external-identities-with-atomic-audit.md) | Authorized external-identity lifecycle with atomic immutable audit |

The range links open the first record in that group; neighboring numbered files
contain the subsequent decisions. Search this directory by concept before
introducing a new record.
