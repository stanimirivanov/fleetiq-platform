# M02, M03 and M04 delivery sequence

Status: Approved sequence; original PRs 1–10 delivered; persistent installation qualification deferred; PR 13 guest authoring merged; PR 14 artifact storage/admission merged; PR 15 activation/routing merged; PR 16 Teltonika component merged; PR 17 durable stateful replay in progress (issue #323).

## TL;DR

This records the previously agreed cross-repository sequence without renumbering.
PR 7 tenant onboarding/access was deliberately delivered as two reviewable backend
PRs: #288 and #292. Original plan PR 8, semantic definition publication and
discovery, is merged as #295; its
[delivery record](semantic-definition-publication.md) contains the evidence.
PR 9, [asset and communication-device registration](metadata-registration.md),
is merged as #299. PR 10 is [relationship and signal-binding revision commands](metadata-revision-commands.md),
tracked by issue #300 and delivered before installation work. Its delivery record
contains verification evidence. Original PR 11 continues through the deployment
repository; its persistent Kubernetes qualification is deferred pending suitable
infrastructure. Independent M03 work resumes with PR 13.
Completed guide, discovery and administration slices are not new work. Future
slices retain the dependencies and milestone boundaries below.

## Approved sequence

The approved sequence contains **33 PR-sized slices**, based on the [milestone scopes](../../../docs/roadmap/milestones.md), the implemented backend, and the `fleetiq-ux/docs/PLANS.md` (UX execution plan).

| Milestone | PRs | Completion outcome |
|---|---:|---|
| **M02 - Product Foundation** | 10 | Operators can discover permitted tenants, administer access, commission metadata, and run the complete development installation. |
| **M03 - Extensible Device Ingestion** | 8 | Registered Teltonika and custom devices produce durable decoded observations through bounded, recoverable ingestion. |
| **M04 - Semantic Twin and Live UI** | 15 | Observations drive an explorable asset graph, current values, bounded history, live subscriptions, maps, and connected operator procedures. |

This excludes completed scaffolding, authentication, metadata persistence, visual layouts, and harness hardening. It includes basic asset and mapping authoring; advanced ontology editors remain outside this plan. Real alerts belong to **M05**, archival history to **M06**, hardware qualification to **M07**, and production rollout to **M08**.

The numbering below is the recommended merge sequence. Dependencies identify where work can proceed concurrently. Each PR belongs to **one repository**.

For brevity, the following gates apply throughout:

- **P:** `cargo xtask verify --group acceptance --base origin/main`, against migrated PostgreSQL with `SQLX_OFFLINE` unset.
- **O:** `cargo xtask verify --group openapi` for published HTTP changes.
- **C:** Existing platform Compose configuration, startup, readiness, isolation, and graceful-stop checks.
- **U:** `pnpm check`, `pnpm e2e`, and `pnpm verify --group browser-quality`.
- **D:** Deployment qualification introduced in PR 11: real Compose configuration/startup, HTTPS, readiness, isolation, restart, and owned cleanup checks.

Every backend API slice includes its DTOs, OpenAPI definitions, examples, authorization, and relevant persistence together. UX consumes an exact merged contract revision. Each UX slice extends its page objects and user-guide procedures.

### PR 1: Complete the connected operator user guide

**M04 · fleetiq-ux · Available immediately**

- **Objective & Scope:** Complete the previously agreed guide for the connected metadata experience. Keep sample-only operational workflows clearly separated.
- **Target Layers / Files:** `fleetiq-ux/apps/web-e2e` (apps/web-e2e), `fleetiq-ux/tools/user-guide` (tools/user-guide), execution and frontend documentation.
- **Contracts & Boundaries:** Existing APIs only. Cover sign-in/account/sign-out, tenant selection and denial, catalogue navigation, inspector definitions, temporal relationships, source/target bindings, recovery, keyboard use, narrow layouts, and themes.
- **Harness Gate:** U, `pnpm guide:generate`, artifact inspection, and publication qualification against the final source.

### PR 2: Expose the operator’s permitted tenants and capabilities

**M02 · fleetiq-platform · Available immediately**

- **Objective & Scope:** Provide bounded discovery of the authenticated operator’s accessible tenants and current actions.
- **Target Layers / Files:** [access](../../../crates/access), identity ports, PostgreSQL adapters, server handlers, OpenAPI.
- **Contracts & Boundaries:** Add an operator-scoped read such as `/api/v1/me/tenants`. Preserve the existing session response; discovery remains a UI hint, with authorization rechecked on every operation.
- **Harness Gate:** P + O; pagination, cross-actor isolation, actor deactivation, tenant suspension, and grant-revocation tests.

### PR 3: Add tenant selection and capability-aware navigation

**M02 · fleetiq-ux · Depends on PR 2**

- **Objective & Scope:** Replace manual-only tenant entry with a complete bounded selection workflow and capability-aware controls.
- **Target Layers / Files:** `fleetiq-ux/apps/web/src/features/identity` (identity feature), app composition, metadata context controls, generated client, browser journeys.
- **Contracts & Boundaries:** Preserve tenant selection in the URL. Clear superseded evidence on account or tenant changes; never treat enabled controls as authorization.
- **Harness Gate:** U; expired-session, revoked-access, empty catalogue, pagination, URL/history, and account-switch tests.

### PR 4: Expose actor provisioning and lifecycle administration

**M02 · fleetiq-platform · Depends on PR 2**

- **Objective & Scope:** Make existing actor provisioning, activation/deactivation, catalogues, and audit services operable through HTTP.
- **Target Layers / Files:** [identity](../../../crates/identity), server administration handlers, identity PostgreSQL adapters, OpenAPI.
- **Contracts & Boundaries:** Consume exact identity-management evidence. Establish shared mutation origin/CSRF enforcement within this real use case; retain retry and administrator-lockout semantics.
- **Harness Gate:** P + O; wrong-action, concurrent retry, rollback, deactivation, lockout, and CSRF tests.

### PR 5: Expose external-identity administration

**M02 · fleetiq-platform · Depends on PR 4**

- **Objective & Scope:** Provide explicit identity attachment/detachment, current mappings, and bounded audit review.
- **Target Layers / Files:** [identity](../../../crates/identity), identity adapters, server identity handlers, OpenAPI.
- **Contracts & Boundaries:** Preserve exact authority/subject matching, prohibit implicit rebinding, and protect the final identity. Keep subjects out of routine diagnostics.
- **Harness Gate:** P + O; identity collision, case sensitivity, last-identity protection, retry, and redaction tests.

### PR 6: Expose platform-authority administration

**M02 · fleetiq-platform · Depends on PR 4**

- **Objective & Scope:** Deliver platform grant/revoke commands, current grant review, and immutable audit pages.
- **Target Layers / Files:** [identity](../../../crates/identity), access policy adapters, server platform handlers, OpenAPI.
- **Contracts & Boundaries:** Preserve action-typed platform authorization and final-administrator protection. Tenant grants never imply platform authority.
- **Harness Gate:** P + O; wrong-action, delegation, revocation, lockout, concurrent command, and cursor tests.

### PR 7: Expose tenant onboarding and access administration

**M02 · fleetiq-platform · Depends on PRs 4 and 6**

- **Objective & Scope:** Deliver the complete tenant onboarding workflow: provision, grant/revoke tenant access, suspend/reactivate, and inspect corresponding audit evidence.
- **Target Layers / Files:** [identity](../../../crates/identity), tenant/access PostgreSQL adapters, server tenant administration, OpenAPI.
- **Contracts & Boundaries:** Keep provisioning, lifecycle, and tenant-access permissions distinct. Reuse existing atomic services; add bounded current-grant reads where needed.
- **Harness Gate:** P + O; atomic onboarding, exact retries, suspension, revocation, cross-tenant denial, and audit ordering.

### PR 8: Publish and discover immutable semantic definitions

**M02 · fleetiq-platform · Depends on PR 4’s mutation boundary**

- **Objective & Scope:** Add bounded definition/version catalogues and publication for asset, property, and relationship types.
- **Target Layers / Files:** [metadata](../../../crates/metadata), PostgreSQL definition repositories, protected-definition handlers, OpenAPI.
- **Contracts & Boundaries:** Preserve immutable versions and existing exact GETs. Define exact retry versus conflicting publication before exposing writes.
- **Harness Gate:** P + O; immutable-content collisions, referenced versions, pagination, authorization, and tenant isolation.

### PR 9: Expose asset and communication-device registration

**M02 · fleetiq-platform · Depends on PR 8**

- **Objective & Scope:** Register generic assets, typed external identifiers, and communication endpoints through authorized APIs.
- **Target Layers / Files:** [metadata](../../../crates/metadata), asset/device PostgreSQL adapters, protected asset/device handlers, OpenAPI.
- **Contracts & Boundaries:** Internal identity remains separate from IMEI or serial numbers. Document the existing device GET; define mutation retry, collision, and audit behavior.
- **Harness Gate:** P + O; atomic identifiers/endpoints, duplicate identities, invalid definitions, ambiguous outcomes, and foreign-tenant references.

### PR 10: Expose relationship and signal-binding revision commands

**M02 · fleetiq-platform · Depends on PRs 8 and 9**

- **Objective & Scope:** Append and correct temporal relationships and exact source-to-property bindings through HTTP.
- **Target Layers / Files:** [metadata](../../../crates/metadata), relationship/binding repositories, server handlers, OpenAPI.
- **Contracts & Boundaries:** Preserve effective/known times, revision conflicts, immutable lineage, and tenant-safe references. Publish existing exact-revision reads alongside commands.
- **Harness Gate:** P + O; concurrent append, temporal boundaries, explicit correction, wrong-source/target, and mutation audit tests.

### PR 11: Compose the complete development installation

**M02 · fleetiq-deploy · Can start immediately**

- **Objective & Scope:** Run the production UX build, backend, PostgreSQL, and configurable OIDC connection behind one HTTPS origin. Establish the deployment repository’s harness within this usable slice.
- **Target Layers / Files:** New `local/compose.yaml`, edge configuration, safe configuration examples, verification scripts, CI, `AGENTS.md`, and operations documentation.
- **Contracts & Boundaries:** Pin backend/UX artifacts; keep backend/database private. Preserve cookies and Origin, avoid API caching, and support configurable local ports.
- **Harness Gate:** D; malformed/partial configuration, TLS, private ports, readiness, container restrictions, restart, and cleanup controls.

### PR 12: Qualify real browser identity and connected metadata

**M04 · fleetiq-deploy · Depends on PRs 7, 9, and 11**

- **Objective & Scope:** Prove browser sign-in and metadata use against the real installation, rather than intercepted API responses.
- **Target Layers / Files:** Deployment browser scenarios, controlled OIDC provider fixtures, authorized synthetic-data setup, qualification scripts, and CI.
- **Contracts & Boundaries:** Exercise existing login/session/logout and metadata APIs. Reuse POM behavior; keep provider fixtures test-only and retain real signature/TLS validation.
- **Harness Gate:** D plus real-browser login/read/logout, CSRF, expiry, revocation, cross-tenant denial, and scanned diagnostics.

Full-profile Kubernetes qualification and D06 are deferred by the owner on
2026-10-25 until suitable infrastructure is available. They remain incomplete;
no capacity gate or installation acceptance is waived. Resume independent M03
work with PR 13; dependent deployment/browser acceptance remains deferred.

### PR 13: Deliver guest-package authoring and conformance tooling

**M03 · fleetiq-platform · Can start alongside M02**

- **Delivery:** Merged through issue #315 / PR #316. CI passed after compatible dependency patches and the explicitly approved exact OIDC advisory exception; no gate was waived.
- **Objective & Scope:** Provide reproducible guest builds, a custom-protocol example, and conformance tests. Add the versioned identity-claim handoff required for device onboarding.
- **Target Layers / Files:** [protocol-sdk](../../../crates/protocol-sdk), protocol-wasm WIT, new package examples/build tooling, author documentation.
- **Contracts & Boundaries:** Preserve existing ABI compatibility. Packages propose identity claims; only the host establishes tenant/device context and execution authority.
- **Harness Gate:** P; actual built-component admission, identity claims, fragmented framing, state, reply gates, and rejected-package controls.

### PR 14: Store protocol artifacts and control admission

Merged through issue #317 / PR #318. Activation and D06 remain separate.

**M03 · fleetiq-platform · Depends on PR 13**

- **Objective & Scope:** Store immutable, bounded artifacts by digest and provide a protected admission workflow, including compilation isolation.
- **Target Layers / Files:** [ingestion](../../../crates/ingestion), protocol-wasm, artifact-storage adapter, server configuration/handlers, OpenAPI.
- **Contracts & Boundaries:** Reuse the existing release catalogue. Registration, admission, and activation remain distinct; guest content grants no host capabilities.
- **Harness Gate:** P + O + C; digest mismatch, immutable collision, interrupted storage, forbidden imports, and compilation/resource limits.

### PR 15: Activate packages and route registered devices

Merged through issue #319 / PR #320; D06 remains deferred.

**M03 · fleetiq-platform · Depends on PRs 9 and 14**

- **Objective & Scope:** Configure exact admitted releases for devices and resolve validated session handoffs.
- **Target Layers / Files:** [ingestion](../../../crates/ingestion), routing/activation persistence, server commissioning handlers, OpenAPI.
- **Contracts & Boundaries:** Treat identifiers as untrusted claims. Define channel/device admission policy, tenant suspension, release pinning, retirement, and active-session behavior.
- **Harness Gate:** P + O; unregistered claims, cross-tenant routing, inactive releases, activation races, revocation, and suspension tests.

### PR 16: Implement the first Teltonika package

Merged through issue #321 / PR #322. Bounded TCP Codec 8/8 Extended component; listeners, hardware and D06 remain separate.

**M03 · fleetiq-platform · Depends on PR 13; parallel with PRs 14–15**

- **Objective & Scope:** Implement an explicitly documented Teltonika handshake and AVL codec subset as a real component.
- **Target Layers / Files:** New Teltonika package, component build configuration, synthetic wire fixtures, conformance suite, support documentation.
- **Contracts & Boundaries:** Emit protocol observations and provenance, without business-asset attribution. IMEI is not authentication; preserve durability-gated ACK proposals.
- **Harness Gate:** P and component conformance; CRC/length errors, fragments, coalesced frames, unknown IO, bounds, and ACK conditions.

### PR 17: Persist decode work and recover stateful processing

Merged as PR #324 (issue #323). D06 and full-profile qualification remain deferred.

**M03 · fleetiq-platform · Depends on PRs 14 and 16**

- **Objective & Scope:** Add durable replay inputs and a restart-safe decoding worker before opening the network listener.
- **Target Layers / Files:** [ingestion](../../../crates/ingestion), PostgreSQL ingestion migrations/adapters, server worker lifecycle.
- **Contracts & Boundaries:** Persist bounded pre-frame state, exact execution identity/context, and sequencing. Define crash-safe state/output commit ordering; historical recovery never replies onto a new socket.
- **Harness Gate:** P + C; lease contention, ambiguous commits, stateful replay equivalence, crashes between stages, poison input, and duplicate completion.

### PR 18: Accept bounded TCP sessions with durable ingestion

Implementation tracked by issue #327. D06 and full-profile qualification remain deferred.

**M03 · fleetiq-platform · Depends on PRs 15–17**

- **Objective & Scope:** Accept real socket traffic, run pinned protocol turns, persist results, and release replies at their required durability stage.
- **Target Layers / Files:** [server](../../../crates/server), ingestion orchestration, protocol host, configuration, local Compose.
- **Contracts & Boundaries:** Host owns sockets, buffering, identities, quotas, and shutdown. Explicitly distinguish internal exact retry from device retransmission across connections.
- **Harness Gate:** P + C; real TCP/PostgreSQL journeys, slow peers, overload, disconnect/commit boundaries, reconnect, and graceful shutdown.

### PR 19: Expose ingestion diagnostics and measure bounded load

**M03 · fleetiq-platform · Depends on PRs 17–18**

- **Objective & Scope:** Provide authorized package/device/worker status and qualify resource use, throughput, and backpressure.
- **Target Layers / Files:** [ingestion](../../../crates/ingestion), server diagnostics, operational read models, OpenAPI, measured fixtures.
- **Contracts & Boundaries:** Distinguish device silence from worker failure or lag. Provide bounded observed-source discovery without exposing raw payloads or credentials.
- **Harness Gate:** P + O + C; redaction, authorization, queue limits, failure/backpressure controls, and a documented repeatable load envelope.

### PR 20: Deliver device onboarding and ingestion diagnostics UI

**M03 · fleetiq-ux · Depends on PRs 3, 9, 15, and 19**

- **Objective & Scope:** Deliver one complete device console for registration, package assignment, connection state, observed sources, and processing diagnostics.
- **Target Layers / Files:** New `features/devices/{model,api,ui}`, app navigation, generated client, device POM and guide procedures.
- **Contracts & Boundaries:** Consume published device/ingestion APIs. Connectivity remains distinct from asset condition and observation freshness; features remain independent.
- **Harness Gate:** U; denied actions, package conflicts, missing sources, stale status, cancellation, narrow layouts, and onboarding journeys.

### PR 21: Run durable semantic processing

**M04 · fleetiq-platform · Depends on PR 17; can develop alongside TCP work**

- **Objective & Scope:** Process claimed decoded outputs through the existing semantic resolver and history processor.
- **Target Layers / Files:** [history](../../../crates/history), PostgreSQL work/progress adapters, server worker composition.
- **Contracts & Boundaries:** Persist event-time and knowledge choices. Retain unresolved outcomes; retries cannot silently select newer bindings or overwrite original interpretations.
- **Harness Gate:** P + C; multi-worker claims, missing bindings, corrections, ambiguous append, crash/restart, and tenant suspension.

### PR 22: Expose bounded property history and provenance

**M04 · fleetiq-platform · Can develop alongside PR 21**

- **Objective & Scope:** Publish asset/property-scoped hot-history queries and exact interpretation provenance needed by the inspector.
- **Target Layers / Files:** [history](../../../crates/history), PostgreSQL history queries, server handlers, OpenAPI.
- **Contracts & Boundaries:** Introduce explicit telemetry-read authorization. Preserve numeric precision, immutable attribution, bounded cursors, and source lineage; archival queries remain M06.
- **Harness Gate:** P + O; late data, multiple interpretations, corrected metadata, pagination, precision, and cross-tenant denial.

### PR 23: Expose bounded semantic graph exploration

**M04 · fleetiq-platform · Depends on PR 10**

- **Objective & Scope:** Return explorable subgraphs with assets, typed edges, and bindings under explicit traversal budgets.
- **Target Layers / Files:** [metadata](../../../crates/metadata), semantic query ports, PostgreSQL graph queries, server/OpenAPI.
- **Contracts & Boundaries:** Preserve effective/known cutoffs and legal cycles. Report truncation explicitly; avoid implying complete graphs or requiring a graph database.
- **Harness Gate:** P + O; cycles, high fan-out, depth/size limits, consistent cutoffs, and tenant-safe traversal.

### PR 24: Materialize durable current state and snapshots

**M04 · fleetiq-platform · Depends on PRs 21–22**

- **Objective & Scope:** Build rebuildable current property state with atomic revisions and authorized snapshots.
- **Target Layers / Files:** A new application-owned projection capability, [live contracts](../../../crates/live), PostgreSQL projection adapters, server projector.
- **Contracts & Boundaries:** Keep `live` a contract crate. Define late-data precedence, quality, freshness, correction behavior, checkpoints, and committed snapshot revisions.
- **Harness Gate:** P + O + C; rebuild equivalence, out-of-order data, atomic revisions, duplicate processing, and restart recovery.

### PR 25: Connect inspector readings, provenance, and hot history

**M04 · fleetiq-ux · Depends on PRs 22–24**

- **Objective & Scope:** Replace unavailable operational panels with complete property evidence and bounded historical exploration.
- **Target Layers / Files:** `fleetiq-ux/apps/web/src/features/assets` (assets feature), connected inspector/controllers, history presentation, generated client, POM.
- **Contracts & Boundaries:** Missing, stale, denied, and unresolved values remain distinct. Preserve exact definitions, units, times, and provenance; charts have accessible table parity.
- **Harness Gate:** U; malformed/unsafe values, stale results, cancellation, empty history, permissions, responsive layouts, and inspector procedures.

### PR 26: Deliver authorized resumable live updates

**M04 · fleetiq-platform · Depends on PR 24**

- **Objective & Scope:** Implement one initial live transport with bounded delivery, reconnect continuity, and explicit resnapshot recovery.
- **Target Layers / Files:** [server](../../../crates/server), projection ports/outbox, live contracts, transport ADR and published protocol.
- **Contracts & Boundaries:** Define transport choice in this slice. Preserve committed cursors, bounded queues, session expiry/revocation, projection versions, and retention-gap behavior.
- **Harness Gate:** P + O + C; real transport reconnects, gaps, slow consumers, authorization changes, cancellation, and buffer limits.

### PR 27: Connect live subscriptions and recovery in the inspector

**M04 · fleetiq-ux · Depends on PRs 25–26**

- **Objective & Scope:** Deliver fine-grained live updates with complete lifecycle and recovery behavior.
- **Target Layers / Files:** `fleetiq-ux/apps/web/src/features/assets` (assets feature), production stream adapter, live controllers, shared runtime where appropriate.
- **Contracts & Boundaries:** Reconcile snapshots/deltas, duplicates, gaps, and resnapshots. Dispose streams on account/tenant/asset changes; keep Effect execution outside presentation.
- **Harness Gate:** U; real transport fixtures, dropped connections, late messages, disposal, bounded reconnect, and high-rate rendering/resource measurements.

### PR 28: Expose observed locations and fleet summaries

**M04 · fleetiq-platform · Depends on PRs 19 and 24**

- **Objective & Scope:** Provide the operational read models needed by the map and overview.
- **Target Layers / Files:** Projection application/query ports, [PostgreSQL adapters](../../../crates/postgres), server fleet/location handlers, OpenAPI.
- **Contracts & Boundaries:** Define configured location-property mapping, coherent coordinate-pair provenance, timestamps, quality, bounded locations, and scoped aggregate counts. Connectivity never implies asset health.
- **Harness Gate:** P + O; incompatible coordinate pairs, missing/stale positions, invalid coordinates, aggregate scope, precision, and query bounds.

### PR 29: Deliver the connected geodetic map workbench

**M04 · fleetiq-ux · Depends on PR 28**

- **Objective & Scope:** Replace the schematic canvas with a MapLibre adapter and configured self-hosted basemap, preserving list/selection/context parity.
- **Target Layers / Files:** `fleetiq-ux/apps/web/src/features/assets/ui/map` (map UI), location models/adapters, map lifecycle owner, browser scenarios.
- **Contracts & Boundaries:** Consume observed geodetic positions. Missing positions remain selectable; tile configuration is deployment-owned. Keep SDK calls isolated and map code route-loadable.
- **Harness Gate:** U; coordinate validation, map/list/URL parity, allowed network destinations, WebGL cleanup, SDK footprint review, and measured map workloads.

### PR 30: Connect fleet overview and operational navigation

**M04 · fleetiq-ux · Depends on PRs 25, 28, and 29**

- **Objective & Scope:** Connect overview summaries, data coverage, and location context while preserving the approved visual hierarchy.
- **Target Layers / Files:** `fleetiq-ux/apps/web/src/features/assets/ui/overview` (overview UI), summary adapters/models, route composition, POM and guide.
- **Contracts & Boundaries:** Use server-owned aggregates rather than accumulated page counts. Keep alert, utilization, and health claims unavailable until their defined M05 counterparts exist.
- **Harness Gate:** U; partial/stale summaries, scope consistency, navigation/history, responsive layouts, and representative fleet-size measurements.

### PR 31: Add asset and relationship commissioning to the registry

**M04 · fleetiq-ux · Depends on PRs 3, 8–10, and 23**

- **Objective & Scope:** Deliver complete asset creation and temporal relationship workflows, including forms, conflicts, cancellation, and authoritative refresh.
- **Target Layers / Files:** `fleetiq-ux/apps/web/src/features/assets` (assets feature), connected registry sections, command adapters, POM and guide.
- **Contracts & Boundaries:** Select discoverable exact definition versions. Preserve relationship direction, tenant ownership, explicit temporal semantics, and audit references.
- **Harness Gate:** U; denied writes, stale revisions, ambiguous outcomes, double submission, navigation cancellation, and commissioning journeys.

### PR 32: Add signal-binding authoring and attribution review

**M04 · fleetiq-ux · Depends on PRs 10, 19, and 31**

- **Objective & Scope:** Deliver the full source-to-property mapping workflow with explicit append/correction and lineage review.
- **Target Layers / Files:** `fleetiq-ux/apps/web/src/features/assets` (assets feature), source/target evidence panels, mapping forms, adapters, POM.
- **Contracts & Boundaries:** Use registered/observed source discovery and pinned target properties. Empty binding results do not establish sensor inventory; corrections create new revisions.
- **Harness Gate:** U; exact-source scoping, incompatible properties, temporal conflicts, denied writes, cancellation, and mapping procedures.

### PR 33: Qualify the installed device-to-web experience

**M04 · fleetiq-deploy · Depends on the preceding ingestion and connected UI slices**

- **Objective & Scope:** Prove the complete path from synthetic device traffic through durable decoding, semantic attribution, live state, and the real browser installation.
- **Target Layers / Files:** Deployment composition, pinned artifacts, package/map mounts, device simulators, installed browser journeys, CI and operator procedures.
- **Contracts & Boundaries:** Use real sockets, backend, PostgreSQL, workers, and streams without API interception. Keep fixtures synthetic and separate from M07 hardware qualification.
- **Harness Gate:** D; receive-to-display provenance, restart recovery, live resnapshot, mapping corrections, revocation, map rendering, resource bounds, and scanned evidence.

Parallel work can begin immediately with **PR 1, PR 2, PR 11, and PR 13**. After the shared administration boundary, identity, grant, definition, and deployment work can proceed independently. Teltonika development can run alongside artifact activation; semantic workers and read models can develop against durable fixtures while TCP work continues. Once backend contracts merge, inspector, mapping, map, and overview teams can work on separate complete slices.

By M04’s end, **overview, map, inspector, and registry** gain connected operational capabilities. The **Alerts** concept retains its completed visual preview and explicit unavailable production state until M05 provides real evaluation and lifecycle semantics.

