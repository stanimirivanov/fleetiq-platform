# ADR 0011: Host-gated protocol replies

- Status: accepted
- Date: 2026-09-11

## Context

Protocol packages must sometimes reply to devices. An identification handshake
may require a response after successful parsing, while a telemetry
acknowledgement may assert that a frame has been durably accepted. Releasing the
latter before the durability condition succeeds can cause a device to discard
data that FleetIQ may still lose.

Custom package code must not write to network sockets directly. Direct access
would bypass durability, output-size, backpressure, auditing, and connection
ownership policies. The package instead needs to propose bytes and declare the
condition under which the trusted host may release them.

The durable log and complete ingestion orchestrator are not implemented yet. A
focused contract can still distinguish the two release paths, make deferred
bytes unavailable to transport code, and carry evidence into the released
reply without selecting a broker or transport framework.

## Decision

The `fleetiq-protocol-sdk` crate defines `ProposedReply`. It contains nonempty
opaque reply bytes and an explicit `ReplyRelease` condition:

- `AfterProcessing` allows release after the package invocation succeeds and
  host validation passes;
- `AfterDurableAcceptance` requires the triggering frame to become an
  `AcceptedMessage` first.

The package cannot access a socket through this contract. `AfterProcessing`
does not promise an immediate write; host scheduling, connection state, and
backpressure still control transport output.

The `fleetiq-protocol-host` crate defines `ReplyGate` with a nonzero hard
per-reply byte limit. The gate consumes a proposal and produces `StagedReply`:

- a processing-gated proposal becomes a ready `OutboundReply`;
- a durability-gated proposal becomes `DeferredReply`.

`DeferredReply` deliberately provides no payload accessor. Releasing it consumes
the deferred value and requires an `AcceptedMessage`. The resulting
`OutboundReply` exposes the bytes to trusted transport code and retains the
accepted-message ID as durable-acceptance evidence. Routine debug output reports
reply lengths and safe metadata without printing wire bytes.

As established by the accepted-message contract, the Rust type cannot itself
prove that storage is durable. Only the ingestion host may construct or publish
an `AcceptedMessage` after its configured durability condition succeeds.

This slice also cannot prove that the supplied accepted message came from the
same frame that caused the reply proposal. The future ingestion orchestrator
must keep a frame, its proposed replies, and the result of its durable append in
one operation context. Recording the accepted-message ID makes the release
decision observable and auditable.

The package's selected release condition is part of its reviewed protocol
semantics and conformance fixtures. A package that labels a data-acceptance
acknowledgement as `AfterProcessing` is incorrect and must not pass activation.
Future manifests may declare acknowledgement capabilities independently of the
wire-level action.

## Consequences

- Protocol packages can express handshake responses and durability-dependent
  acknowledgements without owning network connections.
- Transport adapters receive bytes only from `OutboundReply`, giving the host a
  focused point for connection, backpressure, and audit policy.
- A deferred reply cannot expose its bytes through its public API before the
  release transition.
- Released durability-gated replies retain the accepted-message ID used as
  evidence; processing-gated replies have no such ID.
- Empty replies are rejected because the absence of a reply should be represented
  by producing no action.
- The host applies a consistent per-reply byte limit across packages.
- A package allocates proposed bytes before host staging. A future isolated
  runtime must also enforce guest-memory and aggregate-output limits during
  execution.
- Protocol conformance review remains necessary because package code assigns the
  semantic release condition.

## Alternatives considered

### Let protocol packages write directly to sockets

This bypasses the host's durability gate and prevents consistent resource,
backpressure, connection-ownership, and audit policy.

### Represent every reply as plain bytes

Plain bytes lose the distinction between a handshake response and an
acknowledgement that makes a durability promise. Callers could release both at
the same unsafe point.

### Defer every reply until durable acceptance

Handshake and session-control replies may occur before there is an accepted data
frame. Requiring unrelated durable storage would add latency and can deadlock
protocol progress.

### Use a boolean durability flag

A named release-condition enum states the behavior at call sites and can evolve
more clearly than a boolean whose meaning depends on parameter order or comments.

### Send the reply from the durable-log adapter

The log adapter should establish durability, not own live transport sessions.
Keeping those responsibilities separate lets the ingestion orchestrator join
their results explicitly.

## Out of scope

- durable-log selection and append implementation;
- an ingestion orchestrator that pairs frames, appends, and replies;
- proof that an accepted message corresponds to a particular proposal;
- socket writes and partial-write handling;
- connection ownership and routing across gateway nodes;
- reply queues, backpressure, retries, and write timeouts;
- aggregate output limits across several replies;
- package activation and protocol conformance infrastructure;
- device-command requests and response correlation;
- runtime memory, fuel, and execution-time enforcement;
- a stable native or WebAssembly ABI;
- protocol-specific reply encoders.
