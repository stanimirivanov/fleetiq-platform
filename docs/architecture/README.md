# Architecture

FleetIQ is an asset-centric platform. Its core model supports multiple industries
and treats locomotives as one prospective configuration rather than a universal
domain assumption.

The principal boundaries are:

1. A protocol layer turns bytes from an identified communication device into
   source observations while preserving provenance.
2. A semantic layer binds source observations to typed properties of physical
   assets for an effective time interval.
3. A telemetry layer stores current and historical observations and supplies
   bounded queries.
4. An application layer implements asset management, calculations, alerts,
   commands, and user-facing workflows.
5. Adapters integrate transports, persistence, brokers, identity providers, and
   object storage without leaking them into the domain model.

The initial runtime topology and technology choices remain subject to focused
ADRs and measured prototypes. See the source redesign documents until their
accepted parts are migrated into this repository.

## Semantic asset foundation

An `Asset` is an industry-neutral physical or logical thing owned by one tenant.
Its configurable asset type supplies domain meaning; FleetIQ does not compile a
fixed list such as vehicle, locomotive, motor, or sensor into the core aggregate.

External identifiers consist of a kind, an optional issuing authority, and a
value. VIN, IMEI, serial number, and operator number can therefore coexist
without making any one identifier universal. The asset aggregate prevents exact
duplicates. PostgreSQL enforces cross-asset uniqueness within the tenant, kind,
authority, and value namespace.

Asset-type definitions are immutable and versioned. An asset references a
specific version, preventing a later configuration change from silently changing
its meaning. Each asset-type version also pins the version of every supported
property definition.

Property definitions are reusable across asset types and declare a storage-level
value kind plus an optional canonical unit for numeric values. Calculators can
therefore target semantic property IDs rather than vendor-specific fields. Unit
identifiers are configurable so adopting a standard vocabulary does not require
changing the core domain model.

Relationships are directed, typed domain facts between assets. Relationship-type
definitions are immutable and versioned. Each relationship revision records:

- its tenant and stable relationship identity;
- its recorded revision and the time FleetIQ learned that revision;
- its source and target asset roles;
- the exact relationship-type version;
- a half-open effective interval describing when it applies physically.

This distinction supports delayed information and corrections without silently
rewriting physical history. The domain interval enforces local ordering. Rules
that require the broader graph—permitted asset types, temporal overlap,
cardinality, and containment cycles—belong to the application service and
repository transaction that can inspect that graph. PostgreSQL composite foreign
keys enforce that both endpoints and the exact relationship-type version belong
to the relationship's tenant.

Communication capability is attached to an existing asset rather than modeled as
a competing device identity. A communication device can expose an ordered set of
stable endpoints. Each endpoint has an identity scoped by its device, an
extensible kind, and an operator-facing label. Examples can include onboard
measurements, a CAN interface, a serial channel, or an identified peripheral;
the core model does not prescribe a hardware topology.

Endpoints locate a source within a device. They do not contain protocol package
selection, vendor parameter selectors, or target asset/property attribution.
Device profiles and the protocol runtime interpret vendor fields. Effective-time
signal bindings assign those decoded source signals to semantic asset properties.

A signal binding is a revisioned attribution from a decoded source signal to an
exact semantic property on a target asset. Its source consists of the device
asset, one endpoint beneath that device, and a signal ID emitted by a protocol
package or device profile. Its target consists of the observed asset and an exact
property-definition version. A half-open effective interval and independent
recorded revision preserve historical attribution when commissioning information
arrives late or is corrected.

The domain contract intentionally permits a device to observe its own properties
as well as properties of other assets. Repository and application boundaries
verify tenant membership, endpoint existence, and target property support.
PostgreSQL now enforces those reference and compatibility rules and stores
immutable, contiguous binding revisions. Its bitemporal source lookup selects
the latest revision known at a recorded-time cutoff and applies that revision's
source and effective interval, so corrections do not leave stale attribution.
Concurrent binding appends lock the mutable identity before reading its current
immutable revision in a new `READ COMMITTED` statement.
Cross-binding policies, unit conversion definitions, and authority among
multiple sources require separate explicit contracts before telemetry
normalization uses the bindings.

## Accepted protocol message contract

The `fleetiq-telemetry` crate represents a complete framed message after the
ingestion host's durability condition succeeds. The accepted message retains a
stable identity, admitted tenant and communication-device identity, the
protocol-package release pinned to the session, host receipt time, and immutable
wire bytes. It does not assign an endpoint or source signal before decoding; one
frame may contain records from several endpoint-scoped sources.

The payload remains protocol-owned and may be empty when a custom protocol gives
that frame meaning. Transport and runtime hosts enforce framing, size, memory,
and execution limits. Routine Rust debug output reports payload length without
printing the bytes.

The application layer represents the pre-durability value as
`PendingAcceptedMessage` and owns the `AcceptedMessageLog` port. A stable message
identity is assigned before append so a caller can retry an ambiguous failure
with the same candidate. A first append and an identical already-present value
both return the durably accepted message; reusing the identity with different
immutable content is an error. Concrete storage, serialization, retention, and
the precise durability acknowledgement remain adapter decisions.

PostgreSQL stores accepted messages in a separate `ingestion` schema as
tenant-scoped, immutable rows. The composite message identity and device foreign
key retain structural tenant isolation. Exact package provenance, signed
Unix-millisecond receipt time, and opaque `BYTEA` wire content support replay.
Row-level updates and deletes are rejected; future retention and partition
lifecycle require an explicit forward migration. The append-only application
port does not yet justify secondary replay indexes.

See [ADR 0004](../adr/0004-accepted-message-contract.md) and
[ADR 0029](../adr/0029-append-accepted-messages-idempotently.md), and
[ADR 0030](../adr/0030-store-accepted-messages-as-immutable-ingestion-facts.md).

## Bounded incremental protocol framing

The `fleetiq-protocol-host` crate owns unread transport input for each protocol
session and enforces a hard byte limit. Protocol packages inspect a borrowed
view and return only `NeedMoreData` or the nonzero length of the first complete
frame. The host validates that decision, extracts one frame, and retains any
coalesced input for the next poll.

This boundary supports length-prefixed binary frames, delimited structured
messages, and custom framing rules without assuming that transport reads align
with messages. Appends that exceed the limit fail without changing the buffer.
Incomplete input, package failures, and invalid reported lengths also preserve
the unread bytes so a higher-level session policy can decide how to respond.

An extracted `WireFrame` is still pre-admission input. It becomes an
`AcceptedMessage` only after the host satisfies its durability condition. The
framing contract does not yet select the runtime, define a stable decoder trait
or WebAssembly ABI, or specify identification, session state,
acknowledgements, and commands.

See [ADR 0009](../adr/0009-bounded-incremental-protocol-framing.md).

## Bounded protocol session state

The protocol host owns an opaque state value for each connection session and
enforces a hard retained-byte limit. A protocol package receives a borrowed view
of the current value and may keep it or propose a complete replacement. The host
installs a valid replacement atomically; package failures and oversized
replacements preserve the previous state.

This state lets custom packages represent handshakes, negotiated options, and
other multi-frame parsing context without retaining mutable package instances.
It is ephemeral and is not a durable workflow or replay record. Empty
replacement bytes explicitly clear it, and routine debug output exposes only
its length.

Package-controlled state never establishes trusted tenant or communication-
device identity. Admission context, credentials, and the package release pinned
to a session remain separate host-owned data. A future isolated runtime must
also bound guest memory and execution; the retained-state limit only controls
what the host accepts after an invocation.

See [ADR 0010](../adr/0010-bounded-host-controlled-protocol-session-state.md).

## Host-gated protocol replies

Protocol packages propose nonempty opaque reply bytes instead of accessing
transport sockets. Every proposal states whether it may be released after
successful processing and host validation or must wait until the triggering
frame has been durably accepted. The host applies a hard per-reply size limit
before staging either path.

Processing-gated replies become available to trusted transport code after
validation. Durability-gated replies remain behind a `DeferredReply` with no
payload accessor. Releasing one requires an `AcceptedMessage`, produces an
`OutboundReply`, and records the accepted-message ID as release evidence.

The accepted-message type represents the host's durability contract; its Rust
constructor cannot verify storage. The future ingestion orchestrator must also
ensure the accepted message and reply belong to the same input frame. Package
conformance fixtures must verify that data-acceptance acknowledgements select
the durability-gated path.

See [ADR 0011](../adr/0011-host-gated-protocol-replies.md).

## Two-stage protocol package invocation

Protocol package work has two semantic stages. During framing, a package
borrows the host's unread transport bytes and current opaque session state but
can return only a frame-boundary decision. It cannot change state, emit decoded
values, or propose replies, so repeated inspection of incomplete input has no
package-requested effects.

After extracting one complete frame, the host lends that frame and the state in
effect before processing to the package. The package returns one aggregate with
ordered decoded outputs, one whole-state action, and ordered reply proposals.
Empty output and reply sequences support handshake, keepalive, and control
frames. Output order remains part of deterministic observation identity; reply
order determines transport write order once each proposal's release condition
is satisfied.

The host validates the complete aggregate before applying any part. Trusted
tenant, device, package-release, acceptance, and receipt-time provenance never
enters package-controlled invocation data. These Rust types define semantic and
conformance behavior; WebAssembly serialization, memory transfer, function
names, versioning, and host orchestration remain separate contracts.

See [ADR 0018](../adr/0018-two-stage-protocol-package-invocation.md).

## Atomic frame-processing validation

The protocol host applies explicit limits to every complete package processing
result before later ingestion or transport code can observe it. The limits bound
decoded-output count, reply count, individual reply bytes, and combined reply
bytes. The connection's `SessionState` retains its independent state-byte
ceiling.

The host stages ordered replies locally through the existing reply gate and
applies the proposed state action last. A validation failure drops all local
outputs, exposes no replies, and preserves the previous state. Success returns
the ordered decoded outputs and replies classified by their remaining release
condition. This is an in-memory validation boundary rather than a transaction
over durable storage and transport.

Decoded-output count does not attempt to estimate Rust heap use. The future
isolated-runtime adapter must separately limit the serialized result and decode
variable-length fields under explicit bounds.

See [ADR 0019](../adr/0019-atomic-frame-processing-validation.md).

## WebAssembly invocation interruption experiment

The `fleetiq-protocol-wasm` crate proves that the host can stop runaway guest
code without first defining the protocol package ABI. Its Wasmtime engine
enables deterministic fuel consumption and host-driven epoch interruption. Each
probe invocation receives a nonzero fuel allowance and wall-clock allowance in
a fresh store.

Fuel exhaustion and epoch deadline interruption are reported separately from
ordinary guest traps and runtime failures. Successful probes report consumed
fuel. A shared epoch ticker provides the external time signal; deadlines have
the tick's resolution and may overshoot through operating-system scheduling.

The experiment exposes no WASI or custom host imports. Fuel and epochs interrupt
WebAssembly instructions but cannot cancel a blocking synchronous host call.
Asynchronous integration, ticker supervision, compiled-module caching, and the
real protocol component interface remain separate work.

See [ADR 0012](../adr/0012-wasmtime-invocation-interruption.md).

## Explicit WebAssembly stack limits

Each `WasmSandbox` requires a nonzero maximum WebAssembly stack size and applies
it to its Wasmtime engine. All modules and stores using that engine share the
same approximate stack ceiling. Recursive guest calls that cross it return a
typed error containing the configured value rather than an undifferentiated
runtime trap.

This engine setting limits stack consumed while executing WebAssembly. It does
not reserve native thread stack, guarantee that the calling thread has the
configured capacity remaining, or prevent future host functions from exhausting
native stack. Production execution still requires controlled worker stack sizes
and process-level isolation.

See [ADR 0016](../adr/0016-explicit-wasm-stack-limits.md).

## WebAssembly linear-memory limits

Every WebAssembly probe invocation also receives a nonzero maximum guest
linear-memory size. A store-owned resource limiter checks initial allocation and
runtime growth, permits only one linear memory, and turns a policy violation into
a typed error containing the configured and attempted byte counts.

The current runtime does not enable shared memories and explicitly disables
memory64. Guest memory grows in 64 KiB pages, so non-page-aligned limits permit
only the whole pages that fit. A request at the limit succeeds; initial
allocation or `memory.grow` beyond it traps the invocation.

This policy limits WebAssembly linear memory rather than all memory attributable
to a package. Compiler allocations, Wasmtime metadata, native stacks, code,
virtual address reservations, and host buffers still require measurement and
process-level controls.

See [ADR 0013](../adr/0013-wasm-linear-memory-limits.md).

## WebAssembly table resource limits

Each probe invocation also receives a nonzero maximum number of guest table
elements. The store limiter applies it to initial table allocation and runtime
growth. Exceeding the limit traps with a typed error containing the configured
and attempted element counts; a table exactly at the limit remains valid.

The current runtime permits one table, making the element ceiling an aggregate
invocation limit. Modules declaring more tables fail admission with their actual
and supported table counts. The store repeats the count restriction as defense
in depth. This policy limits guest table storage rather than total instance or
process memory.

See [ADR 0015](../adr/0015-wasm-table-resource-limits.md).

## Bounded WebAssembly module admission

Every compilation requires a `ModuleAdmissionPolicy` with a nonzero maximum raw
module size. The sandbox checks this limit before Wasmtime parses or compiles the
input. Oversized input reports the configured and actual byte counts, while
input exactly at the limit remains eligible for compilation.

Wasmtime validation and compilation failures retain their original error source.
After compilation, the sandbox also rejects modules that declare host imports,
because the current runtime exposes no capabilities. This reports a permanent
compatibility problem during admission rather than waiting for instantiation on
a device-message path.

Admission also requires a host-selected temporary probe export with the exact
`() -> ()` function shape. The resulting `CompiledModule` retains Wasmtime's
module-bound export index, and invocation no longer accepts an independently
selected name. Missing exports, non-function exports, and incompatible arity are
reported before the module can reach message processing. This probe remains an
experiment rather than the stable protocol package ABI.

The raw-byte cap does not bound compilation time, compiler memory, generated
native code, or aggregate activation load. Production activation still requires
isolated workers, bounded concurrency, measurement, and lifecycle policy.

See [ADR 0014](../adr/0014-bounded-wasm-module-admission.md) and
[ADR 0017](../adr/0017-admitted-wasm-probe-export.md).

## Bounded opaque WebAssembly payload transfer

The Wasmtime experiment can also bind a host-selected memory, allocator, and
processing function during module admission. The allocator accepts an opaque
input length and returns its guest pointer. The processor accepts that pointer
and length and returns an `i64` containing an unsigned 32-bit output pointer and
unsigned 32-bit output length.

Input and output have independent host-owned byte ceilings. Input is rejected
before guest execution when it exceeds its limit. Guest-provided pointer-length
ranges are widened and validated against current linear memory before every
host read or write, and output length is checked before host allocation. Safe
Wasmtime memory APIs perform the copies.

Every call uses a fresh bounded store and instance. Dropping that instance
reclaims all guest allocation on success and failure, so the experiment does
not add a guest deallocator that could itself trap after fuel or deadline
exhaustion. The returned bytes remain opaque: serialization, fixed export names,
ABI versioning, and package SDK generation require separate decisions.

See [ADR 0020](../adr/0020-bounded-opaque-wasm-transfer.md).

## Versioned protocol-package component contract

The first protocol-package compatibility boundary is the WIT package
`fleetiq:protocol-package@0.1.0`. A package implements one exported interface
with side-effect-free `locate-frame` and atomic `process-frame` operations. Both
receive host-owned payload and opaque session-state bytes. Framing returns only
a need-more-data decision or a frame length. Processing returns ordered decoded
outputs, one complete state action, and ordered reply proposals.

The interface represents arbitrary-precision decimal values as base-ten text;
the host adapter must parse and validate that text before constructing semantic
SDK values. It also revalidates nonzero frame lengths, identifiers, units,
timestamps, nonempty replies, collection counts, and byte totals. WIT types
describe transport shape and do not replace host policy or domain constructors.

Package errors contain a package-defined numeric code. This gives metrics and
session policy a bounded stable value while diagnostics and package-specific
meaning remain package-release metadata. Traps and host resource failures remain
separate runtime errors.

Wasmtime generates typed host bindings from the checked-in WIT source during a
normal build. The component's embedded world type is checked during admission
against the exact generated host bindings. This contract selects the Component
Model canonical ABI;
the earlier core-module byte-transfer path remains a resource-safety experiment.

The component adapter copies borrowed framing and processing inputs into the
owned values required for canonical ABI lowering. In the return direction, it
converts generated WIT values through the existing SDK and domain constructors.
Zero frame lengths, malformed endpoint/signal/unit identifiers, invalid decimal
text, incompatible value/unit pairs, and empty replies fail conversion. Failures
identify the decoded-output or reply position while omitting the rejected value.
Collection order, timestamps, quality, state action, and reply release conditions
are preserved for the existing host gates.

Component admission uses an independent nonzero raw-byte policy and rejects
oversized input before Wasmtime parsing or compilation. A valid Component Model
binary must declare no host imports and must implement the exact WIT world used
to generate the host bindings. Admission creates typed pre-instantiation
bindings and stores them in an opaque compiled artifact, moving interface type
checking and export-index discovery away from device-message processing.

Compilation is not covered by invocation fuel or epoch deadlines. Activation
must eventually isolate compilation capacity and bound concurrent work.

Typed pre-instantiation does not execute component start functions. Each framing
or processing operation therefore creates a fresh bounded store and component
instance, and its invocation budget covers instantiation and execution. The
component package and canonical ABI adapter require an allowance of two core
instances while memory and table growth retain their independent ceilings.

Package rejections keep their stable code and resource report, semantic
conversion failures identify the violated SDK invariant, and Wasmtime failures
use the shared resource-aware invocation classification. Successful processing
output remains a proposal; applying host policy and committing its effects are
separate work.

See [ADR 0022](../adr/0022-versioned-protocol-package-component-contract.md) and
[ADR 0023](../adr/0023-validate-lifted-component-values.md), and
[ADR 0024](../adr/0024-admit-exact-protocol-components.md), and
[ADR 0025](../adr/0025-invoke-protocol-components-in-fresh-bounded-instances.md).

The WebAssembly adapter also composes `locate-frame` with the bounded
`SessionInputBuffer`. It supplies current unread bytes and current session state
to a fresh component invocation, then lets the host validate and remove at most
one frame prefix. Need-more-data and all failures preserve unread input; valid
coalesced input remains available for subsequent polls.

See [ADR 0027](../adr/0027-poll-component-framing-through-host-input-buffer.md).

The WebAssembly adapter composes successful component processing with the
trusted `FrameProcessingGate`. The integrated operation accepts an extracted
`WireFrame` and the mutable host-owned `SessionState`, constructs the component
input from that same state object, and exposes output only after the complete
proposal passes host limits. Package rejection, component failure, and host
validation failure preserve the previous session state. Successful results
contain accepted decoded outputs, staged replies, and the invocation resource
report; durability and transport release remain later pipeline stages.

See [ADR 0026](../adr/0026-apply-component-output-through-host-processing-gate.md).

The component adapter exposes one bounded session turn that composes these two
host-controlled boundaries. Framing and processing retain independent budgets
and resource reports. Incomplete input ends the turn without processing. A
complete frame is processed immediately against the same session state used for
framing, and a successful turn retains the wire frame for later durable
acceptance. If processing fails, the prefix remains consumed from the input
buffer but ownership of the exact frame moves into the error so session policy
can retain, retry, or quarantine it explicitly. Each turn handles at most one
frame, leaving scheduling of coalesced input to the caller.

See [ADR 0028](../adr/0028-run-one-bounded-component-session-turn.md).

## Host-controlled protocol decoding boundary

The `fleetiq-protocol-sdk` crate is the first shared boundary between protocol
packages and their host. A package emits an ordered sequence of
`DecodedOutput` values. Each output contains only information that decoding can
legitimately establish: the endpoint and stable source signal within the
admitted device, the validated typed measurement, and an optional device
timestamp.

The trusted host combines each output with its `AcceptedMessage`. This assembly
adds the tenant and communication-device identity established by admission, the
host receipt time, the pinned package release, and a zero-based output index.
Package code therefore cannot attach decoded data to another tenant or device,
rewrite host time, or claim another package's provenance. Output order is part
of deterministic replay and must remain stable for the same message and package
release.

This Rust API defines semantic and conformance behavior shared by native tests
and future sandboxed packages. It is not a stable plugin ABI. Framing, session
state, acknowledgements, package manifests, serialization, execution limits,
and the native or WebAssembly host interface remain later runtime decisions.

See [ADR 0005](../adr/0005-host-controlled-protocol-decoding.md).

## Decoded observation contract

The `fleetiq-telemetry` crate defines the boundary between protocol decoding and
semantic attribution. A decoded observation carries a tenant admitted by the
host, a device/endpoint/signal source, one typed value, an optional numeric unit,
source quality, timestamp evidence, and exact protocol-package provenance. It
does not contain a target asset property; temporal signal binding supplies that
meaning in the next pipeline stage.

The host assigns an accepted-message identity after satisfying its durability
condition. A decoded observation identity combines that message identity, the
immutable protocol-package release, and the output's zero-based position. The
same input and package release can therefore be replayed idempotently, while a
deliberate reinterpretation through another package release remains a distinct
fact with visible provenance.

Device-recorded time remains optional and separate from host receipt time. The
decoder preserves both even when the device clock appears to be ahead; a later
timestamp policy selects event time and quality using operational evidence.
Durable raw payload storage, event serialization, unit conversion, source
authority, and semantic observation persistence remain separate contracts.

## Semantic resolution

The `fleetiq-semantics` resolver consumes a decoded observation through focused,
consumer-owned binding and property-definition read views. Blanket adapters use
the application repositories, while workers may provide local projections
without implementing unrelated writes. Callers provide both the effective time
used for physical attribution and the inclusive knowledge cutoff used for
historical interpretation. The resolver does not infer event time from device
and receipt timestamps.

Exactly one effective binding must select the decoded source. Its exact target
property definition must exist, and the decoded value kind and source unit must
equal the property's required kind and canonical unit. A successful result
retains decoded provenance, the semantic target, the binding identity and
revision, and the bitemporal resolution context.

No binding, competing bindings, missing semantic metadata, and kind or unit
incompatibility are structured unresolved results. They preserve the decoded
observation for diagnostics and later retry. Repository failures are operational
errors. Unit conversion, source authority, timestamp selection, and policy for
whether uncertain or invalid values update asset state remain later explicit
increments.

## Asset-property change contract

The `fleetiq-semantics` crate turns a successfully bound observation into an
`AssetPropertyChange` only after the caller explicitly selects device-recorded
or host receipt time as the event-time basis. Selecting device time fails when
the observation did not carry that evidence. Selecting receipt time remains an
explicit choice even when device time exists. The contract does not infer clock
trust, substitute timestamps silently, or discard either original timestamp.

An asset-property change retains the complete bound observation, including
source and decoder provenance, exact semantic target, binding revision, and
resolution context. Its deterministic identity combines the decoded observation
identity, binding identity and revision, and event-time basis. Reinterpreting an
observation through a corrected binding therefore creates a distinct semantic
change without changing the underlying decoded fact.

The change is an update candidate for history, latest-state projection, and
calculators. It does not claim that its value differs from prior state or that
it should replace current state. Source authority, timestamp trust, value
quality acceptance, ordering, tie-breaking, freshness, and sparse-update rules
remain explicit downstream policies.

See [ADR 0006](../adr/0006-asset-property-change-contract.md).

## Calculator output contract

The `fleetiq-analytics` crate defines calculator evaluation and output contracts
without selecting a worker runtime or algorithm. The host creates a logical
evaluation with tenant identity, a state-scope asset, an exact calculator
release, event time, evaluation time, and an identity that must remain stable
when the same configuration and input selection are replayed.

Calculator logic emits `CalculatedProperty` values. Each value identifies its
target asset and exact property-definition version and must match that
definition's value kind and canonical unit. A calculator may target an asset
other than its scope asset so an aggregate or subsystem calculation can publish
its result where the semantic model declares it.

The host assembles each calculated property with trusted evaluation context and
a zero-based output index. The resulting identity combines evaluation identity,
calculator release, and output position. Output order is therefore part of the
replay contract. An evaluation may validly produce no outputs.

Event time identifies the logical instant or window boundary evaluated;
evaluation time records when processing occurred. The evaluation ID is the link
to durable configuration and input evidence rather than copying a potentially
large observation window into every output. Calculator definitions, input
declarations, evidence persistence, state, windows, late-data behavior,
checkpoints, and algorithms remain later contracts.

See [ADR 0007](../adr/0007-calculator-output-contract.md).

## Revisioned snapshots and live updates

The application crate defines transport-independent live delivery contracts
that can carry any authorized projection payload. A `ProjectionSnapshot`
contains the complete visible state at one projection-scoped revision. A
`ProjectionDelta` declares both the revision it requires and the later revision
that results after its ordered changes are applied. Deltas may coalesce several
commits, but must always advance the revision.

A consumer starts its cursor at the snapshot revision and accepts a delta only
when the delta's base equals that cursor. A mismatch leaves the cursor unchanged
and requires recovery rather than guessing over a gap. The consumer advances
the cursor only after applying the delta successfully.

Snapshot creation and subscription must share a consistency barrier. An
implementation may subscribe before reading the snapshot and buffer bounded
deltas, or replay a durable outbox after the snapshot revision. In either case,
the first delivered delta must continue from the snapshot cursor so no change
can fall between the snapshot and live stream.

Bounded delivery never accumulates an unlimited backlog for a slow consumer.
When required deltas are unavailable, `ResnapshotRequired` directs the consumer
to discard local projection state and obtain a fresh authorized snapshot.
Reasons distinguish lag, revision mismatch, authorization change, and projection
reset. An authorization change stops delivery from the old visibility scope
before rebuilding the projection.

Revisions provide continuity within one projection and are not globally
comparable domain revisions. Transport selection, serialization, payload change
types, queue sizes, resume retention, heartbeat behavior, and authorization
implementation remain later decisions.

See [ADR 0008](../adr/0008-revisioned-live-projection-contract.md).

## Metadata persistence

PostgreSQL migrations define the persisted metadata schema. Handwritten domain
aggregates remain independent of that schema. Application-owned repository ports
separate use cases from SQLx adapters, whose private row types reconstruct domain
objects through validated constructors.

Tenant-aware composite primary and foreign keys enforce isolation structurally.
Assets reference an asset-type definition from the same tenant and exact version.
External identifier uniqueness is scoped by tenant, identifier kind, optional
authority, and value. Aggregate writes include identifiers in one transaction.

The semantic-definition repository stores and retrieves exact property,
asset-type, and relationship-type versions. Asset-type definitions persist their
property references atomically and retain declaration order. Published
definition versions are append-only: PostgreSQL rejects updates and deletes so a
semantic change must introduce a new version.

Asset relationships have stable identities with immutable, contiguous recorded
revisions. Each revision stores directed endpoints, an exact relationship-type
version, recorded time, and a half-open effective interval. A mutable head
serializes concurrent appends; deferred constraints verify the complete revision
sequence and monotonic recorded time at commit. Snapshot queries accept separate
effective-time and recorded-time cutoffs so corrections do not rewrite what was
known previously. The repository locks only the mutable head before reading its
dependent immutable revision in a new `READ COMMITTED` statement, preventing a
pre-lock-wait join snapshot from producing stale concurrency decisions.

Communication-device rows reuse an existing asset's tenant and identity.
Tenant-aware foreign keys prevent capability from being attached across tenant
boundaries. Endpoint identities are unique within a device and retain declaration
order during aggregate persistence and reconstruction. Device registration and
all endpoints are written atomically with one set-based PostgreSQL statement.

See [ADR 0001](../adr/0001-postgresql-sqlx-persistence.md) and
[ADR 0021](../adr/0021-lock-revision-head-before-dependent-read.md).
