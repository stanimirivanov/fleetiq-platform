# Current implementation state

## TL;DR

- The [durable semantic worker](../operations/semantic-worker.md) now queues committed decoded outputs, pins binding selection and atomically appends history with fenced progress. Unresolved/exhausted outcomes remain retained; public history, durable current-property snapshots and authorized exact-property SSE now exist.

- [Bounded TCP ingestion](../operations/tcp-ingestion.md) now accepts explicitly
  configured identity-v1 channels through isolated live execution and fenced durable
  results. Identifier routing is not device authentication; D06 remains deferred.

- [Stateful protocol replay](../operations/protocol-replay.md) now persists exact
  accepted/pre-state inputs and fenced claims, and commits decoded effects and
  checkpoints atomically through an isolated Linux worker. Historical recovery never replies;
  D06 and full-profile qualification remain deferred.

- Kubernetes plan D02 adds source-owned immutable backend image qualification and explicit maintainer publication; no environment deployment is performed. See [image production](../operations/image-publication.md).

- Kubernetes plan D01 adds [explicit migration and checked runtime commands](../operations/runtime-and-migrations.md), deterministic embedded migration inventory and bounded mounted OIDC CA trust. Image publication, Kubernetes packaging, release/recovery orchestration and real deployed browser qualification remain later slices.

- [Relationship and signal-binding commands](../operations/metadata-revisions.md)
  now expose attributed immutable appends and explicit corrections; their existing
  exact revision reads are included in the published HTTP baseline.

- The repository includes the first runnable backend composition root and a
  provider-neutral, PostgreSQL-backed identity and tenant authorization
  boundary. The `fleetiq-access` crate owns authentication and
  authorization policy and evidence; `fleetiq-identity` owns the actor, grant,
  tenant, and bootstrap use cases and ports. PostgreSQL supplies their adapters.
  The `fleetiq-metadata` crate owns asset-graph use cases and ports;
  `fleetiq-ingestion` owns durable acceptance, decoded-result persistence,
  and package-catalogue ports; `fleetiq-history` owns exact durable-observation
  semantic processing and the property-change append and exact read ports. Asset,
  semantic-definition, relationship, communication-device, and
  signal-binding operations consume exact
  authorization evidence. Configured-credential HTTP exact asset, bounded
  asset catalogue, communication-device, exact definition,
  relationship-revision, signal-binding-revision,
  bounded asset-relationship snapshot, bounded source-binding snapshot, and
  bounded target-asset binding snapshot queries use those boundaries;
  actor administration now exposes provisioning, lifecycle, catalogue, and audit
  HTTP operations; external-identity and platform-grant administration also have
  authenticated HTTP operations. Tenant provisioning, lifecycle, catalogue and
  lifecycle audit now have [HTTP administration](../operations/tenant-administration.md)
  under distinct exact platform permissions. Tenant grant/revoke, current grants
  and audit now require exact tenant access.manage independently of platform grants.
  The authenticated caller can
  discover bounded active tenants and exact current actions through
  [operator tenant discovery](../operations/operator-tenants.md); hints never
  replace independent per-operation authorization.
- Asset definitions, assets, relationships, devices, signal bindings, accepted
  messages, decoded results, semantic property changes, and protocol releases
  have PostgreSQL contracts.
- Protocol framing and processing are bounded and a versioned WIT component
  boundary is admitted and invoked through Wasmtime.
- Semantic resolution, calculator output, and `fleetiq-live` projection
  contracts exist; property-change append and exact reads are durable, while production
  calculator stores remain planned; current-property materialization and authorized SSE are durable-state consumers.
- The complete development stack runs through Compose on isolated loopback
  ports; production deployment remains a later milestone.
- The HTTP surface exposes version discovery and bounded problem details under
  `/api/v1`, plus optional authenticated and authorized exact asset, bounded
  asset catalogue, device, asset-type-definition, property-definition,
  relationship-type-definition,
  exact relationship- and signal-binding-revision reads, and a bounded
  bitemporal relationship snapshot for one asset, a bounded source-binding
  snapshot for one decoded signal, and a bounded binding snapshot targeting one
  asset.
- Exact platform authorization is distinct from tenant-scoped access, and an
  authorized command provisions a tenant, its initial access, and immutable
  audit evidence atomically.
- Platform administration now consumes action-typed evidence; a grant for one
  platform action cannot be routed to another protected use case.
- A one-time atomic bootstrap establishes the first platform operator, exact
  external identity, platform-access, identity-management, and
  tenant-provisioning grants, and immutable root audit fact;
  an explicit one-shot process command invokes it without opening an HTTP route.
- A backend-owned OpenAPI 3.1 UI baseline documents version discovery,
  bounded asset catalogue and exact asset reads, pinned semantic definitions,
  and bounded relationship and signal-binding snapshots. It remains a partial
  contract: live semantic state and some
  protected routes are not yet described. Browser OIDC login and operator
  sessions, tenant discovery, actor administration, external-identity
  administration, platform-grant administration, tenant provisioning/lifecycle,
  and tenant-access administration are included in the
  additive baseline.
- Immutable semantic definitions now have protected publication and bounded
  collection/version catalogues; exact metadata.manage and metadata.read remain
  separate. See [commissioning](../operations/semantic-definitions.md).
- Asset and communication-device registration now expose create-only HTTP commands with exact metadata.manage, atomic immutable actor/time evidence and content-checked retries; see [registration](../operations/metadata-registration.md).
- Further business APIs, identity-provider qualification, device transport listeners, operator UI, archival storage, and
  observability remain future milestones.

## Browser operator authentication

Provider-neutral OIDC authorization-code login and same-origin server sessions
are implemented. The backend validates signed identity tokens, resolves an
already provisioned actor, stores opaque credential digests in PostgreSQL, and
rechecks current actor mapping and tenant grants for every metadata read.
Cookie, expiry, callback replay, origin/CSRF, trust-domain reconfiguration, and
rollback policies are in the
[browser identity procedure](../operations/browser-identity.md).
The UX connection remains the next slice; this backend boundary does not
qualify a customer provider or a public production deployment.

## Semantic metadata foundation

The domain and PostgreSQL adapters support:

- tenant-scoped immutable versions of asset types, property definitions, and
  relationship types;
- assets with internal identity, a pinned asset-type version, and typed external
  identifiers;
- revisioned, effective-dated relationships between tenant-owned assets;
- communication devices and ordered extensible endpoints; and
- revisioned, effective-dated signal bindings from device sources to exact
  asset property versions.

Repository operations enforce tenant-safe references, append order, bitemporal
selection, and validated row-to-domain reconstruction. The current model does
not prescribe locomotive, road-vehicle, refrigeration, or other industry
schemas.

## Protocol contracts and host policy

The protocol SDK and trusted host implement:

- bounded incremental framing over fragmented and coalesced input;
- bounded opaque connection state with atomic whole-state replacement;
- proposed replies with processing or durability release conditions;
- a two-stage framing and complete-frame processing contract;
- atomic validation of output counts, reply counts and bytes, and state bytes;
- ownership-preserving failure values for retry; and
- a bounded session turn that extracts and processes at most one frame.

The host has no production network listener or device-session registry yet.
The source-built [Teltonika package](../../packages/teltonika/README.md) supports
a bounded IMEI handshake and TCP Codec 8/8 Extended subset, retaining source
timestamps and raw unknown IO. Actual component conformance covers host ACK gates;
hardware compatibility and durable worker/listener qualification remain pending.

## WebAssembly protocol components

The Wasmtime integration implements:

- raw artifact limits before compilation;
- rejection of undeclared host imports and unsupported table shapes;
- deterministic fuel and external wall-clock interruption;
- explicit approximate guest stack, linear-memory, and table limits;
- validated pointer-length transfer for the earlier core-module experiment;
- a versioned WIT world for framing and processing;
- exact-world admission with typed pre-instantiation bindings;
- host validation of lifted values; and
- fresh bounded stores and component instances for each invocation.

Admitted components retain their immutable package release and host-computed
SHA-256 artifact digest. PostgreSQL catalogue registration is implemented
separately from admission, including exact retry, immutable-content collision,
tenant-scoped lookup, and the complete unsigned artifact-size range. Device/channel activation and internal scoped routing are implemented;
transport listeners and package distribution remain planned. Source-built Rust guest authoring and conformance are implemented in the
[custom protocol example](../../examples/custom-protocol/README.md). An opt-in
identity world returns bounded, redacted external identity proposals; resolving
and authorizing those claims remains a separate host routing responsibility.

## Durable ingestion

The `fleetiq-ingestion` application capability and PostgreSQL adapters
implement two append-only boundaries:

1. accepted messages retain tenant, device, package release, receipt time, and
   exact wire bytes; and
2. decoded results retain one completion marker and the complete ordered set of
   typed observations for that accepted message.

Both distinguish first append, exact retry, and immutable identity collision.
Decoded marker and child rows append atomically, including zero-output results.
Ingestion orchestration withholds decoded values and durability-gated replies
until their required append succeeds and returns exact pending work on failure.
A PostgreSQL recovery harness covers suppressed successful responses and exact
retry across both durability stages.

## Semantic and live contracts

The semantic resolver applies bitemporal bindings and exact property
definitions, returning explicit unresolved outcomes for missing or incompatible
configuration. Successful binding produces asset property change candidates
with event-time basis and deterministic interpretation identity.
Its read ports and snapshot query belong to semantic policy; PostgreSQL
implements the ports directly, and metadata re-exports the query for its
callers. `fleetiq-history` composes the resolver and an append-only
property-change port for one decoded observation with explicit event-time and
knowledge choices. It can load an exact tenant-owned durable observation before
processing it; absence, lookup failure, and unresolved attribution are distinct.
PostgreSQL verifies the candidate against durable decoded and binding facts
and records its exact interpretation identity and event-time basis. The
processor preserves unresolved observations and safe retry input. An exact
history read returns the immutable source, target, event time, and first-append
resolution context without re-resolving corrected metadata. A bounded keyset
page lists all immutable interpretations of one exact decoded output with a
shared source read. The durable semantic worker now owns fenced recovery and
atomic history completion; protected property-window reads remain separate.

Analytics contracts represent versioned calculator evaluations and typed
semantic outputs, plus bounded canonical input manifests, finalized event-time
windows, immutable configuration identities and logical replay keys. Terminal
classifications distinguish completion, insufficient evidence and bounded failure;
persistence and runners must enforce their cross-record invariants. The `fleetiq-live` crate owns full snapshots, forward-only
deltas, cursor continuity, and resnapshot reasons. A tenant-owned immutable first-party calculator registry and checked-schema
`register-calculator` command now persist exact input/output declarations.
Atomic terminal evaluation envelopes now persist pinned input references and
ordered calculated outputs with provenance checks and exact retry comparison; see
[calculator evaluation storage](calculator-evaluation-storage.md).
The checked-schema `calculate-once` command now runs the first-party window mean
with immutable preparation, explicit decimal truncation and atomic terminal append.
The explicit checked-schema [analytics worker](../operations/analytics-worker.md) now
enumerates bounded fixed windows with durable cursors, database-clock claims,
three-attempt fencing and atomic terminal completion. HTTP activation, operator
recovery controls and installed dashboard qualification remain planned. Exact-property subscription transport now uses the authorized SSE boundary.

## Runtime and data

The workspace is Rust 2024 on Rust 1.99.1. PostgreSQL adapters are organized
by access, identity, metadata, ingestion, history and projection capability inside one crate. A
private codec keeps persisted platform and tenant action names consistent
across policy, administration, catalogue, and audit paths. PostgreSQL 18.6
stores metadata and ingestion facts under checked SQL migrations. SQLx macros validate queries
against a migrated database. Committed offline query descriptions permit
database-independent image builds while online checks remain authoritative.

The `fleetiq-server` binary validates environment configuration, applies the
embedded migration chain, exposes liveness and PostgreSQL-backed readiness
probes, identifies the version 1 HTTP contract, returns bounded problem details
for unknown API resources and unsupported methods, and shuts down gracefully
after `Ctrl+C`. A configured bearer identity enables protected exact asset,
bounded asset catalogue, communication-device, asset-type, property-definition,
relationship-type,
exact relationship-revision, exact signal-binding-revision, bounded
asset-relationship snapshot, bounded source-binding snapshot, and bounded
target-asset binding snapshot queries;
without complete credential configuration those routes are absent. It is an
operable development composition root, not a production deployment. Unix deployments
also handle `SIGTERM`, which is the normal container stop signal.

The local Compose deployment builds that binary in a multi-stage image, waits
for PostgreSQL readiness, publishes both services only on loopback, and runs the
backend as a non-root user with a read-only filesystem and dropped Linux
capabilities. It is disposable development infrastructure rather than a
production manifest.

## Identity and tenant access

The domain defines a stable internal actor identity that does not encode an
identity provider. The access authorization service evaluates an explicit
actor, tenant, and action tuple through a consumer-owned policy port. Only an
allow decision produces typed, exact-operation authorization evidence; missing
grants and policy failures fail closed.
Tenant evidence also carries its action in its Rust type, so protected
metadata and access-administration use cases reject incompatible grants at
compile time.

PostgreSQL persists internal actor existence and exact tenant action grants. Its
policy adapter performs one complete-tuple existence query for each operation,
so a deleted grant affects the next decision without cache delay. Constraints
require valid existing actors and tenants, reject unknown actions, and prevent
duplicate grants. Integration tests cover actor, tenant, and action isolation,
revocation, vocabulary alignment, and durable constraints.

Tenant provisioning uses a separate platform authorization scope because no
tenant exists before provisioning succeeds. PostgreSQL persists the exact
`(actor_id, tenant.provision)` grant, and current-state policy evaluation makes
revocation effective on the next decision. Platform grants and tenant grants
do not imply one another.

The provisioning service consumes exact platform evidence and derives the
provisioner identity from it. One transaction records immutable provisioning
evidence, creates the tenant, and grants the existing initial-administrator
actor `metadata.read`, `metadata.manage`, and `access.manage`. Host-assigned
provisioning identity and tenant identity are both unique arbiters. Exact
retries return the durable outcome; changed content is a collision; concurrent
exact commands have one winner; and any failed reference rolls back every row.
The initial platform operator can be established once through a separate
unauthorized bootstrap boundary. One transaction claims an immutable singleton
record, creates the actor and exact external-identity mapping, and grants
the explicit `platform.access.manage`, `identity.manage`, `tenant.provision`,
and `tenant.lifecycle.manage` actions. Exact actor and identity retries resolve
safely,
including under concurrency; another bootstrap identity is rejected; and
dependent-write failure rolls back the claim. The
`bootstrap-platform-operator` process command
validates dedicated environment values, applies migrations, invokes this
capability without binding an HTTP listener, prints only its disposition, and
exits. No HTTP or deployment-job adapter invokes bootstrap.

An actor-provisioning service consumes exact `identity.manage` evidence and
derives its trusted provisioner from that evidence. One transaction creates the
stable internal actor, its exact initial external-identity mapping, and
immutable provisioning evidence. Provisioning ID, actor ID, and external
identity are independent idempotency arbiters; exact retries return the durable
outcome, changed content is a collision, and concurrent exact commands have one
winner. A new actor receives no platform or tenant grant implicitly.

Platform grant administration consumes exact `platform.access.manage`
evidence and derives its trusted administrator from that evidence. Grant and
revoke commands serialize through the bootstrap singleton, mutate current
state, and append immutable audit evidence atomically. Platform-scoped command
identities distinguish exact retries from changed-content collisions, and the
recorded effect distinguishes a state change from an already-satisfied request.
Revocation affects the next policy decision. The final
`platform.access.manage` grant cannot be revoked, preventing administrative
lockout while allowing authority to move after explicit delegation.

Platform-grant administration now has authenticated grant/revoke, actor-scoped
current-grant and immutable audit HTTP routes. Delivery retries retain first
receipt time; revocation preserves the final active administrator. See
[the operating procedure](../operations/platform-access-administration.md).

The same exact platform administration authorization protects bounded,
newest-first audit queries. Stable keyset cursors use recorded time and the
platform-scoped change identity, including deterministic ordering when several
commands share one timestamp. It also protects bounded current-state queries
for one selected actor's exact platform grants. Those pages follow stable
persisted action-name order and remain separate from immutable change history.

Actor lifecycle administration consumes exact `identity.manage` evidence and
atomically activates or deactivates a stable actor with immutable audit
evidence. Deactivation retains external identities, grants, and historical
references while making new exact identity resolution and current tenant or
platform policy evaluation fail closed. Activation restores those retained
paths. Global serialization preserves exact retry behavior and prevents
deactivation of the final active identity administrator or platform-access
administrator. The same exact identity-management authorization protects
bounded, newest-first lifecycle audit queries. Stable keyset cursors use
recorded time and the platform-scoped lifecycle change identity. Previously
issued typed authorization evidence is not recalled. The same exact identity
management authorization protects a bounded platform-wide actor catalogue
ordered by stable actor identity, with explicit active or deactivated current
state and no external-identity subject disclosure.

Actor provisioning, activation/deactivation, bounded actor catalogue, and bounded
lifecycle audit now have HTTP administration routes on configured authenticated
listeners. Each consumes exact current platform identity.manage evidence. Browser
commands share logout's exact-origin/CSRF guard; HTTP deliveries retain the first
durable host receipt time, while existing immutable commands keep strict time
equality. See [ADR 0100](../adr/0100-expose-actor-administration-over-http.md) and
the [operating procedure](../operations/actor-administration.md). Tenant
grant administration, tenant administration, and administration UI remain separate.


Tenant lifecycle administration uses a separate exact
`tenant.lifecycle.manage` platform action. One transaction activates or
suspends the stable tenant boundary and appends immutable audit evidence.
Suspension retains tenant-owned data and grants while fresh tenant
authorization decisions fail closed; activation restores the retained grants.
The bootstrap operator receives the new action explicitly, and later
delegation uses audited platform grant administration. Previously issued typed
authorization evidence is not recalled. The same exact lifecycle-management
authorization protects bounded, newest-first lifecycle audit queries. Stable
keyset cursors use recorded time and the platform-scoped lifecycle change
identity. It also protects a bounded platform-wide tenant catalogue ordered by
stable tenant identity, with explicit active or suspended current state.
Internal protocol handoff eligibility honors current tenant suspension.
Production transport cancellation remains planned.

External identities use an exact trusted-authority and opaque-subject key that
resolves to one stable internal actor. Subjects remain case-sensitive and are
never normalized. One actor may have multiple mappings, but mapping existence
grants no tenant action. A provider-neutral application service now requires a
credential-specific authenticator to validate credentials before resolving the
external identity and producing typed authenticated-actor evidence. Rejected
credentials never reach identity resolution, and valid but unregistered
principals fail closed. Tenant authorization consumes that evidence and builds
its policy request internally, so a bare or caller-supplied actor identifier
cannot enter the public authorization API.

Identity administration consumes exact `identity.manage` evidence and derives
the administrator from it. Attach and detach commands serialize globally,
mutate the exact current mapping, and append immutable audit evidence in one
transaction. Exact retries preserve their original effect, changed-content
identity reuse is rejected, and an external identity already assigned to
another actor is never rebound implicitly. Detach cannot remove an actor's
final external identity. Deferred insertion assertions still require bootstrap
and actor-provisioning evidence to create the exact initial mapping, while that
historical evidence no longer permanently pins a mapping after an authorized
replacement.

The same exact identity-management authorization protects bounded, newest-first
queries over immutable external-identity change evidence. Stable keyset cursors
use recorded time and the platform-scoped change identity. Authorized results
retain the exact authority and opaque subject, while routine Rust debug output
redacts the subject. It also protects bounded current-state queries for one
selected actor's external identities, ordered by exact authority and subject.
Catalogue entry and cursor diagnostics redact the opaque subject, and the query
does not provide platform-wide principal enumeration.

External-identity attachment/detachment, actor-scoped current mappings, and bounded
change history now have protected HTTP routes. Each request rechecks exact current
identity.manage authority. Browser mutations use the shared origin/CSRF guard;
redelivered commands retain the original time and effect without undoing later
mapping changes. Detaching a session's login mapping invalidates its next request.
See [ADR 0101](../adr/0101-expose-external-identity-administration-over-http.md) and
[operations](../operations/external-identity-administration.md).

The application can grant or revoke an exact tenant action only with consumed
`ManageAccess` authorization evidence. Tenant and administrator identity come
from that evidence. Each host-identified command atomically changes current
grant state and appends immutable audit evidence containing its actors, action,
operation, recorded time, and state effect. Exact retries return the original
effect, different-content identity reuse is rejected, and concurrent commands
are serialized per tenant. The same exact authorization protects bounded,
newest-first audit queries. Stable keyset cursors use recorded time and change
identity, and repository tenant scope comes only from authorization evidence.

`fleetiq-metadata` application services now cover assets, immutable semantic
definitions, revisioned relationships, communication devices, and revisioned
signal bindings. Asset and device registration plus relationship and binding
revision append consume exact `ManageMetadata` evidence and reject aggregates
owned by another tenant before persistence. Definition publication derives its
tenant from the same evidence because definitions do not carry tenant identity.
Asset, definition, relationship, graph-snapshot, device, and binding lookup
consume exact `ReadMetadata` evidence and derive repository tenant scope from
that evidence, so callers cannot independently select a tenant. Neither action
implies the other.

In workload mode, one configured opaque bearer credential authenticates a single external
identity for exact asset and bounded tenant asset catalogue reads, the exact
asset's nested `/device` resource, exact asset-type, property-definition, and
relationship-type lookup, exact recorded relationship-revision lookup,
exact recorded signal-binding-revision lookup, a bounded asset-relationship
snapshot, a bounded source-binding snapshot, and a bounded target-asset binding
snapshot. In that mode, the process mounts those routes only when the token, authority, and
subject are configured together. The
current actor mapping and exact `metadata.read` tenant grant are checked on
every request before the metadata repository is called. Browser mode instead
uses the provider-neutral operator sessions described above. Customer provider
and deployment qualification and membership or role models remain future work.
Platform and tenant administration now have protected HTTP adapters. See the
[protected asset query procedure](../operations/asset-query.md),
[device query procedure](../operations/device-query.md),
[asset-type query procedure](../operations/asset-type-query.md),
[property query procedure](../operations/property-query.md),
[relationship-type query procedure](../operations/relationship-type-query.md),
[relationship revision query procedure](../operations/relationship-revision-query.md),
[signal-binding revision query procedure](../operations/signal-binding-revision-query.md),
[asset relationship snapshot procedure](../operations/asset-relationship-snapshot.md),
[signal-binding snapshot procedure](../operations/signal-binding-snapshot.md),
and [asset-binding snapshot procedure](../operations/asset-binding-snapshot.md).

There is no broader business resource API, production authentication provider,
message broker, object archive, cache,
production deployment manifest, or service-level objective. Do not describe
the version-discovery resource, library contracts, or local database defaults
as production product behavior.

## Deliberate limitations

- No Teltonika or other real device package is distributed.
- No package activation, connection routing, network transport, or command path.
- No archival semantic history query or installed end-to-end qualification; exact
  observation processing, source-scoped keyset pages, and append-only
  persistence exist.
- No tenant/user administration, authentication, authorization, or audit UI.
- No live map, graph editor, dashboard, alert delivery, or broad graph query API.
- No archive, retention enforcement, backup/restore, or cost controls.
- No hardware qualification fixtures or field-device interoperability suite.
- No production observability, scaling, rollout, or recovery automation.

The intended sequence is maintained in
[implementation milestones](../roadmap/milestones.md). Update this document when
a merged capability materially changes what the repository can do.

## Tenant-access HTTP administration

Exact tenant access.manage now authorizes grant/revoke commands, an actor's
bounded current grants and tenant-scoped immutable audit. Platform grants never
substitute. Commands preserve first durable receipt time and effect on retries;
strict timestamp commands retain their existing meaning. No schema change is
needed. Final tenant-administrator self-revocation remains possible; no platform
bypass or tenant recovery API is introduced. See the
[operator procedure](../operations/tenant-access-administration.md) and
[execution plan](plans/tenant-access-administration-http.md).

## Semantic definition commissioning

Original plan PR 8 exposes collection GET/POST and per-identity version catalogues
for asset, property and relationship types. Publication resolves exact complete
content on retry and rejects changed content under an immutable version. Existing
exact GETs and strict Rust insert behavior remain unchanged. Catalogues use one
bounded summary query; no migration or attributed publication audit is added.
See [ADR 0106](../adr/0106-publish-and-discover-immutable-semantic-definitions.md)
and the [execution plan](plans/semantic-definition-publication.md).

## Protocol artifact admission

Issue #317 adds tenant-scoped bounded PostgreSQL artifacts and explicit protected
registration/read/admission routes. Admission uses a supervised Linux worker and
retains exact release/world/policy evidence; ordinary metadata authority does not
permit it. Revisioned device/channel activation and scoped identity handoffs are implemented; listeners remain planned. See the
[admission procedure](../operations/protocol-package-admission.md).

## Protocol device routing

Issue #319 adds exact admitted package/world selection per registered device and
host channel, immutable generations, historical retry receipts and explicit
retirement. Internal lookup constrains tenant/issuer/channel/decoder and revalidates
current eligibility before future bounded turns. Routing does not authenticate
peers or activate network listeners. See the [commissioning guide](../operations/protocol-route-commissioning.md).

## Bounded ingestion diagnostics

The ingestion service and PostgreSQL adapter expose an authorized, read-only 64-frame diagnostic snapshot and bounded observed-source discovery. Durable processing states do not imply process/socket health; both liveness fields are explicitly unknown. The [real TCP/PostgreSQL measured fixture](../operations/ingestion-diagnostics.md) records transport/storage throughput, ACK latency and process memory separately from excluded Wasm/vendor costs. D06 and full-profile infrastructure qualification remain deferred.

## Bounded property history and provenance

Delivery plan PR 22 (issue #333) adds exact tenant `telemetry.read`, typed history
services, an indexed set-based PostgreSQL page query with a five-second local
statement timeout, and protected HTTP history/provenance routes. Page sizes are
1–100 and mandatory half-open event windows span at most seven days. Cursors
bind exact target and window; pages preserve late-data limitations, all original
interpretations and precise string-encoded numeric values. See the
[property history guide](../operations/property-history.md). Live delivery,
archive completeness, D06 and full-profile qualification remain deferred.

## Bounded semantic graph exploration

Delivery-plan PR 23 (issue #335) adds metadata-owned graph budgets, traversal
and consistent-read ports, set-based PostgreSQL frontier queries, and protected
HTTP graph reads. Current `metadata.read` authority permits a root-scoped graph
at explicit effective/known cutoffs, with at most four hops, 100 assets and
200 links of each kind. Corrected revisions, cycles, self-links and shared
vertices retain their meaning; truncation is explicit and returned links have
closed endpoints. See [the operations guide](../operations/semantic-graph.md).
D06 and full-profile qualification remain deferred.

## Durable current property state

Delivery-plan PR 24 (issue #337) adds application-owned projection policy and
ports, transactional interpretation enqueue/backfill, bounded per-tenant atomic
batches, event-time/full-identity winner arbitration and consistent authorized
exact-property snapshots. `project-current` and explicit `rebuild-current` process
modes check schema without DDL. Rebuild advances generation/revision and preserves
later pending commits. The [current-state guide](../operations/current-property-state.md)
records role grants, deadlines, freshness, correction limits and recovery.
Inspector live activation is implemented in fleetiq-ui PR #89. D06 and full-profile qualification remain deferred.


## Authorized current-property SSE

Delivery-plan PR 26 (issue #339) adds one exact-property live operation with
scope-bound generation/revision ids, complete coalesced replacements, explicit
resnapshot/terminal controls and fresh authority around reads. The stream is
pull-driven, with bounded connection/payload/time budgets and no new SQL state
or background producer queue. See the [live operations guide](../operations/property-live-updates.md).
Heavy PostgreSQL/transport/Rust execution remains on CI; D06 and full-profile
qualification remain deferred. Inspector activation is implemented in fleetiq-ui PR #89. This is latest-state
synchronization, not lossless history replay or a capacity/deployment qualification.

## Observed fleet locations and coverage

Product-plan PR 28 (issue #341) adds one dual-authorized tenant fleet read with
explicit exact-version coordinate mapping, conservative source/device-time
coherence, exact coordinates, unlocated asset rows and whole-tenant inventory /
projection coverage counts. Reads reuse existing metadata/history/current state
in a bounded repeatable view without new schema or a fleet worker. See the
[fleet snapshot guide](../operations/fleet-location-snapshots.md). Geodetic map
and overview consumers are PRs 29–30. D06 and full-profile qualification remain
deferred; no device-health or connectivity inference is provided.
