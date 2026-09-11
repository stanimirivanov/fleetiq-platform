# ADR 0010: Bounded host-controlled protocol session state

- Status: accepted
- Date: 2026-09-11

## Context

Many telemetry protocols cannot interpret every frame independently. A session
may progress through a handshake, identification, authentication, negotiated
options, partial command exchange, or another package-defined phase. FleetIQ
must support those protocols and customer-authored protocols without embedding
every state machine in the host.

Protocol packages therefore need state that survives between invocations on one
connection. Giving a package unrestricted mutable memory for the lifetime of a
session would make resource enforcement dependent on each implementation and
would couple native and isolated runtimes to different state models. The host
needs a portable semantic boundary that it can bound and later transfer across
an isolation ABI.

Session state is package-specific and may contain sensitive protocol material.
The host does not need to understand its schema. It does need to prevent state
from becoming an unbounded allocation, preserve the previous state when an
invocation fails, and keep trusted admission identity outside package-controlled
bytes.

## Decision

The `fleetiq-protocol-sdk` crate defines `SessionStateAction` with two package
decisions:

- `Keep` preserves the current state exactly;
- `Replace` proposes complete opaque replacement bytes. An empty replacement
  explicitly clears the state.

The `fleetiq-protocol-host` crate defines `SessionState`. It owns the state for
one connection session, initializes it as empty, and configures a nonzero hard
limit for retained state bytes.

When requesting an action, the host passes the package a borrowed view of the
current state. The package cannot mutate the retained bytes in place. If it
returns `Replace`, the host checks the complete replacement length before
atomically installing it. `Keep`, package failure, and an oversized replacement
cannot partially alter the current state.

Session state is ephemeral and scoped to its owning connection. It is not a
durable workflow store, replay record, or cache shared between connections.
Persistent device facts and command workflows use their own explicit contracts.

Opaque state is also not trusted admission context. A package may use it to
remember protocol progress, but the host must separately hold any admitted
tenant, communication-device identity, credentials, and pinned protocol-package
release. Downstream messages receive trusted provenance from that host-owned
context rather than by decoding fields from the opaque state blob.

Routine debug output for both proposed actions and retained state reports byte
lengths without printing contents.

These Rust types define semantic behavior for native tests and a future runtime
adapter. They do not define the serialized representation or memory-transfer ABI
for isolated packages.

## Consequences

- Custom protocol packages can implement handshakes and multi-frame behavior
  without owning transport connections or persistent host memory.
- The maximum retained package state is consistent across protocol
  implementations.
- Whole-state replacement gives each invocation transactional state semantics:
  the previous state remains available unless one valid action completes.
- State formats can remain package-specific and evolve with package releases.
  Sessions must eventually be pinned to a compatible package release.
- The host cannot inspect opaque bytes for operational lifecycle decisions.
  Connection, admission, timeout, and command-routing state that the host needs
  to enforce must be modeled separately.
- The retained-state limit applies after a package returns its action. A future
  isolated runtime must also limit guest memory and output allocation while the
  package is executing.
- Replacing state copies or transfers a complete blob rather than applying
  package-defined mutations. State limits should remain small; profiling can
  inform a different transport representation without changing the semantics.

## Alternatives considered

### Keep mutable state inside each native package instance

This hides resource use from the host and cannot be transferred consistently to
an isolated runtime. A failed invocation may also leave partially mutated state.

### Define a host-understood handshake state machine

Common host lifecycle state is useful, but protocol-specific handshakes and
negotiation cannot be represented by one fixed state machine without moving
vendor behavior into the host.

### Store session state as a host-managed key-value map

A generic map imposes serialization, naming, and mutation semantics before the
requirements of multiple protocols are known. An opaque bounded value provides
the smaller portable boundary.

### Persist every session-state transition

Connection parsing state is temporary and may change for every frame. Durable
storage would add latency and data volume without making ephemeral handshakes a
valid source of business truth. Durable accepted frames and command workflows
have separate requirements.

### Allow in-place package mutation

In-place mutation makes failure atomicity and isolation-boundary transfer more
difficult. Complete replacement lets the host install a state change only after
the package returns successfully and resource validation passes.

## Out of scope

- a complete protocol invocation or decoder trait;
- a stable native or WebAssembly ABI;
- session-state serialization conventions or schema migration;
- transport connections and lifecycle;
- host-owned admission and authentication state machines;
- package manifest capabilities and release pinning;
- persistence or restoration across reconnects;
- sharing state across sessions or nodes;
- guest-runtime memory, fuel, and timeout enforcement;
- framing, decoding, acknowledgements, and commands in one invocation;
- protocol-specific handshake implementations.
