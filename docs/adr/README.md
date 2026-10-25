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
3. Change it to `Accepted` when the change is approved. When reconciling a
   proposed record, name the actual human approval or reviewed merge evidence
   in its status section. Implemented code, passing tests, or authorization to
   start work alone do not accept an architectural decision.
4. Use `Superseded by ADR NNNN` when a later decision replaces it, and record
   the reciprocal `Supersedes` reference in that later record. A later ADR may
   use a qualified `Supersedes` reference for a scoped refinement that does not
   replace the earlier decision's remaining lifecycle.
5. Use `Rejected` for a proposal retained to prevent repeated investigation.
6. When publishing the next record, add its index coverage and advance
   `LATEST_PUBLISHED_ADR` in `crates/xtask/src/repository/adr.rs`. This retained
   high-water mark makes deletion of the newest historical record detectable.

Do not renumber records. Correct spelling or broken links in an accepted ADR,
but do not rewrite its context, decision, alternatives, or consequences to make
history match the current system.

Write required headings, lifecycle and supersession metadata, and index entries
in plain Markdown. Raw HTML comments may explain authoring, but raw HTML
elements must not carry checked ADR fields.

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
| [0057](0057-query-external-identity-audit-with-keyset-pages.md) | Authorized bounded external-identity audit queries with redacted diagnostics |
| [0058](0058-administer-actor-lifecycle-with-atomic-audit.md) | Reversible actor activation with atomic immutable audit and recovery safeguards |
| [0059](0059-query-actor-lifecycle-audit-with-keyset-pages.md) | Authorized bounded actor lifecycle audit queries with stable keyset pagination |
| [0060](0060-administer-tenant-lifecycle-with-atomic-audit.md) | Reversible tenant suspension with exact platform authority and atomic immutable audit |
| [0061](0061-query-tenant-lifecycle-audit-with-keyset-pages.md) | Authorized bounded tenant lifecycle audit queries with stable keyset pagination |
| [0062](0062-query-tenant-catalogue-with-keyset-pages.md) | Authorized bounded tenant catalogue queries with stable keyset pagination |
| [0063](0063-query-actor-catalogue-with-keyset-pages.md) | Authorized bounded actor catalogue queries without external-identity disclosure |
| [0064](0064-query-current-external-identities-by-actor.md) | Authorized bounded discovery of one actor's current external identities |
| [0065](0065-query-current-platform-grants-by-actor.md) | Authorized bounded discovery of one actor's current platform grants |
| [0066](0066-enforce-capability-dependency-policy.md) | Executable package roles and direct workspace dependency policy |
| [0067](0067-enforce-repository-documentation-policy.md) | Executable documentation and repository policy |
| [0068](0068-enforce-supply-chain-policy.md) | Pinned dependency auditing, update review, and fixture-tool alignment |
| [0069](0069-own-semantic-read-ports-in-policy.md) | Consumer-owned semantic read ports without an application dependency |
| [0070](0070-extract-access-policy-boundary.md) | Authentication and authorization policy owned outside broad application orchestration |
| [0071](0071-type-tenant-authorization-evidence.md) | Action-typed tenant authorization evidence and compile-time use-case routing |
| [0072](0072-type-platform-authorization-evidence.md) | Action-typed platform authorization evidence and compile-time use-case routing |
| [0073](0073-extract-identity-application-capability.md) | Identity administration use cases and ports owned by a dedicated application capability |
| [0074](0074-extract-asset-graph-application-capability.md) | Asset graph use cases and ports owned by a dedicated metadata application capability |
| [0075](0075-extract-ingestion-application-capability.md) | Durable ingestion orchestration and ports owned by a dedicated application capability |
| [0076](0076-extract-live-projection-contracts.md) | Revisioned live projection contracts owned by a transport-independent crate |
| [0077](0077-organize-postgres-adapters-by-capability.md) | Capability-aligned PostgreSQL adapter modules and exact persisted action codecs |
| [0078](0078-retire-application-compatibility-facade.md) | Direct capability imports and retirement of the flat application facade |
| [0079](0079-protect-initial-asset-query.md) | Configured bearer credential and first authorized asset query |
| [0080](0080-expose-communication-device-detail.md) | Authorized communication-device detail nested beneath its asset |
| [0081](0081-read-exact-asset-type-versions.md) | Authorized exact-version asset-type definition query |
| [0082](0082-read-exact-property-definitions.md) | Authorized exact-version property-definition query |
| [0083](0083-read-exact-relationship-type-versions.md) | Authorized exact-version relationship-type definition query |
| [0084](0084-read-exact-relationship-revisions.md) | Authorized exact recorded relationship revision query |
| [0085](0085-read-exact-signal-binding-revisions.md) | Authorized exact recorded signal-binding revision query |
| [0086](0086-page-bitemporal-asset-relationships.md) | Authorized bounded bitemporal relationship snapshot for one asset |
| [0087](0087-page-bitemporal-source-bindings.md) | Authorized bounded bitemporal signal-binding snapshot for one decoded source |
| [0088](0088-page-tenant-asset-catalogue.md) | Authorized bounded tenant asset catalogue of summaries |
| [0089](0089-page-bitemporal-asset-target-bindings.md) | Authorized bounded binding snapshot targeting one asset |
| [0090](0090-persist-immutable-semantic-property-changes.md) | Append-only semantic property-change history with exact source lineage |
| [0091](0091-compose-semantic-history-processing.md) | One-observation resolution and durable append with aligned event time and exact retry |
| [0092](0092-process-exact-durable-decoded-observation.md) | Exact tenant-scoped durable observation lookup followed by semantic history processing |
| [0093](0093-read-exact-semantic-interpretation.md) | Exact tenant-scoped semantic history read with immutable source and binding provenance |
| [0094](0094-page-observation-interpretations.md) | Bounded keyset page of one decoded output's immutable semantic interpretations |
| [0095](0095-authenticate-browser-operators-with-oidc-sessions.md) | Provider-neutral OIDC and same-origin browser operator sessions |
| [0096](0096-enforce-external-dependency-ownership-and-lint-inheritance.md) | Exact external dependency ownership, nonmember path rejection, and shared lint inheritance |

| [0097](0097-protect-published-history-and-sensitive-artifacts.md) | Trusted-base migration immutability, pinned redacted secret detection, and policy ownership |
| [0098](0098-coordinate-reproducible-verification-and-evidence.md) | Authoritative verification inventory, explicit groups, reviewed CI generation, and metadata-only result evidence |

| [0099](0099-discover-current-operator-tenant-access.md) | Advisory actor-scoped current tenant and action discovery |

| [0100](0100-expose-actor-administration-over-http.md) | Actor administration HTTP with shared mutation protection and first-receipt retries |

| [0101](0101-expose-external-identity-administration-over-http.md) | Exact external-identity HTTP administration, bounded mapping/audit pages, and first-receipt retries |

| [0102](0102-expose-platform-access-administration-over-http.md) | Platform-grant HTTP administration with first-receipt retries and active recovery protection |

| [0103](0103-expose-tenant-administration-over-http.md) | Tenant provisioning and lifecycle HTTP with distinct platform permissions and safe first-receipt retries |

| [0104](0104-bound-parallel-rust-tests-and-cache-dependencies.md) | Bounded cross-binary Rust tests, explicit doctests, and trusted-main dependency caching |

| [0105](0105-expose-tenant-access-administration-over-http.md) | Tenant-scoped grant HTTP commands, bounded current grants and immutable audit |
| [0106](0106-publish-and-discover-immutable-semantic-definitions.md) | Immutable definition publication with content-checked retries and bounded version discovery |
| [0107](0107-register-assets-and-communication-devices.md) | Atomic asset/device registration with content-checked retries and original actor/time evidence |

| [0108](0108-expose-attributed-metadata-revision-commands.md) | Attributed immutable relationship/binding corrections with exact retry semantics |

| [0109](0109-separate-production-serving-from-schema-migration.md) | Explicit migration execution, no-DDL production serving/bootstrap and additive OIDC CA trust |

| [0110](0110-publish-qualified-immutable-backend-artifacts.md) | Source-owned build-once backend OCI images, migration inventory and explicit maintainer publication |

| [0111](0111-add-opt-in-protocol-identity-handoff.md) | Optional untrusted identity world and source-built guest conformance |

| [0112](0112-store-and-isolate-protocol-admission.md) | Immutable bounded protocol artifacts and isolated policy-versioned admission |

| [0113](0113-activate-revisioned-device-protocol-routes.md) | Revisioned admitted device/channel activation, scoped identity routing and session revalidation |

| [0114](0114-add-bounded-teltonika-protocol-component.md) | Source-built bounded Teltonika TCP decoding, source signals and gated ACK proposals |

The range links open the first record in that group; neighboring numbered files
contain the subsequent decisions. Search this directory by concept before
introducing a new record.
