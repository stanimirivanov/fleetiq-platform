# Current implementation state

## TL;DR

- The repository includes the first runnable backend composition root and a
  provider-neutral, PostgreSQL-backed identity and tenant authorization
  boundary. The `fleetiq-access` crate owns authentication and
  authorization policy and evidence; `fleetiq-identity` owns the actor, grant,
  tenant, and bootstrap use cases and ports. PostgreSQL supplies their adapters.
  The `fleetiq-metadata` crate owns asset-graph use cases and ports;
  `fleetiq-ingestion` owns durable acceptance, decoded-result persistence,
  and package-catalogue ports. Asset, semantic-definition, relationship,
  communication-device, and signal-binding operations consume exact
  authorization evidence. Configured-credential HTTP asset, communication-
  device, and exact definition queries use those boundaries;
  administration remains without an HTTP API.
- Asset definitions, assets, relationships, devices, signal bindings, accepted
  messages, decoded results, and protocol releases have PostgreSQL contracts.
- Protocol framing and processing are bounded and a versioned WIT component
  boundary is admitted and invoked through Wasmtime.
- Semantic resolution, calculator output, and `fleetiq-live` projection
  contracts exist; their production workers and stores do not.
- The complete development stack runs through Compose on isolated loopback
  ports; production deployment remains a later milestone.
- The HTTP surface exposes version discovery and bounded problem details under
  `/api/v1`, plus optional authenticated and authorized asset, device,
  asset-type-definition, property-definition, and relationship-type-definition
  reads.
- Exact platform authorization is distinct from tenant-scoped access, and an
  authorized command provisions a tenant, its initial access, and immutable
  audit evidence atomically.
- Platform administration now consumes action-typed evidence; a grant for one
  platform action cannot be routed to another protected use case.
- A one-time atomic bootstrap establishes the first platform operator, exact
  external identity, platform-access, identity-management, and
  tenant-provisioning grants, and immutable root audit fact;
  an explicit one-shot process command invokes it without opening an HTTP route.
- Further business APIs, identity-provider integration, authorization administration
  APIs, device transport listeners, operator UI, archival storage, and
  observability remain future milestones.

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
tenant-scoped lookup, and the complete unsigned artifact-size range. Activation
policy, artifact storage, compilation isolation, package distribution, and
author tooling remain planned.

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
callers.

Analytics contracts represent versioned calculator evaluations and typed
semantic outputs. The `fleetiq-live` crate owns full snapshots, forward-only
deltas, cursor continuity, and resnapshot reasons. No production semantic worker,
property history store, calculator runtime, subscription transport, or operator
dashboard exists yet.

## Runtime and data

The workspace is Rust 2024 on Rust 1.98.1. PostgreSQL adapters are organized
by access, identity, metadata, and ingestion capability inside one crate. A
private codec keeps persisted platform and tenant action names consistent
across policy, administration, catalogue, and audit paths. PostgreSQL 18.6
stores metadata and ingestion facts under checked SQL migrations. SQLx macros validate queries
against a migrated database. Committed offline query descriptions permit
database-independent image builds while online checks remain authoritative.

The `fleetiq-server` binary validates environment configuration, applies the
embedded migration chain, exposes liveness and PostgreSQL-backed readiness
probes, identifies the version 1 HTTP contract, returns bounded problem details
for unknown API resources and unsupported methods, and shuts down gracefully
after `Ctrl+C`. A configured bearer identity enables the protected asset,
communication-device, asset-type, property-definition, and relationship-type
queries; without complete credential configuration those routes are absent. It
is an operable development composition root, not a production deployment. Unix
deployments also handle `SIGTERM`, which is the normal container stop signal.

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
Production ingestion suspension remains planned.

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

One configured opaque bearer credential authenticates a single external
identity for `GET /api/v1/tenants/{tenant_id}/assets/{asset_id}`, its nested
`/device` resource, and exact asset-type, property-definition, and
relationship-type lookup. The process mounts those routes only when the token,
authority, and subject are configured together. The current actor mapping and
exact `metadata.read` tenant grant are checked on every request before the
metadata repository is called. A production identity provider, multi-principal
login, membership or role model, administration transport, generalized
authentication middleware, and audit HTTP API remain future work. See the
[protected asset query procedure](../operations/asset-query.md),
[device query procedure](../operations/device-query.md),
[asset-type query procedure](../operations/asset-type-query.md),
[property query procedure](../operations/property-query.md), and
[relationship-type query procedure](../operations/relationship-type-query.md).

There is no broader business resource API, production authentication provider,
authorization administration API, message broker, object archive, cache,
production deployment manifest, or service-level objective. Do not describe
the version-discovery resource, library contracts, or local database defaults
as production product behavior.

## Deliberate limitations

- No Teltonika or other real device package is distributed.
- No package activation, connection routing, network transport, or command path.
- No semantic history persistence or end-to-end ingestion worker.
- No tenant/user administration, authentication, authorization, or audit UI.
- No live map, graph editor, dashboard, alert delivery, or query API.
- No archive, retention enforcement, backup/restore, or cost controls.
- No hardware qualification fixtures or field-device interoperability suite.
- No production observability, scaling, rollout, or recovery automation.

The intended sequence is maintained in
[implementation milestones](../roadmap/milestones.md). Update this document when
a merged capability materially changes what the repository can do.
