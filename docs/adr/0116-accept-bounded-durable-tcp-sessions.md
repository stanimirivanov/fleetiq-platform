# ADR 0116: Accept bounded durable TCP sessions

## Status

Proposed, 26 October 2026.

Supersedes: none. Superseded by: none.

## Context

Admitted identity-capable components and durable replay exist independently.
Live ingestion needs a bounded owner of socket, framing, state and reply ordering
without granting protocol code transport, tenant or database authority.

## Decision

An explicit Linux-only `ingest-tcp` process accepts one configured tenant/channel
and immutable identity-v1 package. Activation requires the operator to select
`registered-identifiers`. The identifier claim routes only registered devices;
it does **not authenticate a peer**. Default binding is loopback; deployment must
provide a trusted private ingress boundary. Public device authentication and TLS
termination are separate work, not implicit guarantees of this listener.

The ingestion capability owns connection-local state, proposal validation,
routing revalidation, immutable frame identity and durable sequencing. Socket and
isolated child adapters live in the server. The server never links Wasmtime.
One fixed sibling live executor uses the established bounded private process
supervisor; its separate closed envelope carries identity, source outputs, state
and reply proposals. The parent reconstructs and restages all proposals.
Transferring a deferred reply into IPC preserves its durable release condition;
it never constructs an outbound reply. Historical replay retains its reply-free
wire contract and has no reply port.

Before execution, bootstrap verifies active tenant, exact stored admitted release
and identity world. Before and after every later execution the current route,
claim, admission and content must still match the original pin. Revocation
prevents subsequent turns, but cannot retract an already-started bounded effect.

For each complete frame the host validates the entire batch/state/reply set
before any effect. Processing-only replies may be released first. Raw bytes and
exact replay work then commit together before durability-gated replies. Package
reply order is preserved: a later ready proposal cannot overtake a deferred one.
An exact message-scoped claim then fences decoded effects and checkpoint in one
transaction. Only successful completion advances in-memory state and buffer.
Recovery may win that claim; the live connection then closes without stealing
other work. No database transaction spans child execution or socket writes.

Every connection gets a random 128-bit nonce; sequence-derived message IDs remain
stable within it. Reconnecting creates fresh identities, even for identical wire
bytes. Internal exact retries do not imply cross-connection device deduplication.
Any uncertainty or cancellation ends the socket owner. Accepted work survives for
reply-free recovery; there is no retry onto another socket.

Bounds are eight connections, two per peer, one child, 64 KiB unread input,
1 MiB lifetime input, 256 committed frames and 512 polls. Idle reads expire after
15 seconds, writes after two, turns after 25 and connections after 300. The child
retains the 15-second parent deadline and Linux process ceilings. Shutdown stops
admission, cancels socket owners, drains child cleanup and closes the pool.

```mermaid
sequenceDiagram
    participant Device
    participant Host as Bounded TCP owner
    participant Child as Isolated live component
    participant DB as PostgreSQL
    Device->>Host: Fragmented or coalesced bytes
    Host->>DB: Check admission and current pinned route
    Host->>Child: Unread bytes and exact pre-state
    Child-->>Host: Prefix and untrusted proposals
    Host->>DB: Revalidate route
    Host->>Host: Validate all effects; preserve reply order
    Host-->>Device: Eligible processing-only replies
    Host->>DB: Commit raw frame and replay work
    Host-->>Device: Remaining durability-gated replies
    Host->>DB: Exact fenced claim; atomic outputs and checkpoint
    Host->>Host: Advance state and consumed prefix
```

## Consequences

This establishes a real TCP-to-durable-source path without migrations, brokers,
public device credentials or changes to the HTTP contract. Teltonika and custom
identity-v1 packages use the same orchestration. Initial admission is rechecked
by the child; compilation per poll favors isolation over throughput. Fixed limits
are conservative functional policy, not measured production capacity.

A raw-durability ACK means the frame can be recovered, not that decoded or semantic
processing completed. Processing-only ACKs retain their weaker package contract.
Disconnected or uncertain clients may retransmit and produce distinct accepted
facts; downstream deduplication needs an explicit device protocol contract.
Diagnostic APIs/load measurement follow in PR 19. D06, full-profile qualification,
hardware certification and public ingress remain deferred.

See [TCP operations](../operations/tcp-ingestion.md) and
[durable recovery](../operations/protocol-replay.md).

## Alternatives considered

- Execute Wasmtime in the server: breaks the established isolation boundary.
- Commit state before output, or ACK raw durability before confirmed commit:
  permits lost work and invalid state progression.
- Use one global replay claim for live traffic: may steal unrelated frames.
- Persist replies for reconnect delivery: confuses old work with a new socket.
- Treat an IMEI or custom identifier as authentication: a routing claim is public
  and spoofable; trusted channel deployment must be explicit.
