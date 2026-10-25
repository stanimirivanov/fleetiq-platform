# Custom protocol guest

## TL;DR

- Build an import-free Rust component against FleetIQ's authoritative WIT.
- Run the actual source-built guest through the bounded host conformance test.
- Treat handshake identifiers as untrusted claims; host admission/routing remains required.

## Build and verify

Run from the repository root with the pinned Rust toolchain and installed
`wasm32-unknown-unknown` target (declared in `rust-toolchain.toml`):

```powershell
cargo build --locked --release --target wasm32-unknown-unknown -p fleetiq-custom-protocol-example --target-dir target/protocol-guest
cargo run --locked -p fleetiq-protocol-wasm --example componentize -- target/protocol-guest/wasm32-unknown-unknown/release/fleetiq_custom_protocol_example.wasm target/protocol-guest/custom-protocol.component.wasm
cargo test --locked -p fleetiq-protocol-wasm --test custom_protocol
```

The core module is not the distributable component. The componentizer uses
Bytecode Alliance `wit-component` and validates embedded canonical ABI metadata.
The integration test builds the maintained source, componentizes it and admits
those exact bytes. Missing toolchain/target/build prerequisites fail the test;
there is no ignored test or prebuilt-binary fallback. No WASI adapter is needed. Only the generator/export module has the owner-approved
unsafe allowance; architecture checks reject other unsafe allowances and handwritten
additions inside that generated boundary.

Source, root Cargo.lock, exact Rust target/toolchain, WIT and release profile
are build inputs. Pin all of them when publishing. Paths and target architecture
can affect bytes: do not claim cross-host byte reproducibility without measuring
it. Publication/storage/activation are separate later delivery slices.

## Wire format

Each frame is `[length: u8][opcode: u8][body]`; length counts opcode plus body,
not the length byte. Only lengths 2..33 are accepted. Partial input returns
need-more-data; coalesced input returns only the first frame's size.

| Opcode | Body | Result |
|:--|:--|:--|
| 1 | 1..32 ASCII alphanumeric serial bytes | Propose replacement session state containing the serial; immediate one-byte ACK `01` |
| 2 | Signed big-endian i16; length exactly 3 | With nonempty explicit session state, emit integer `main`/`temperature`, keep state; ACK `01` deferred until durable acceptance |

For example, handshake `04 01 41 42 43` proposes serial `ABC`; sample
`03 02 00 2a` emits integer 42. The example intentionally leaves units and device
recorded time absent. Error code 1 means invalid/unsupported input or missing
handshake state. It is illustrative, not a production device implementation.

## Identity and state

The optional `fleetiq:protocol-package/identity-protocol-package@0.1.0` world
includes the unchanged runtime and an identification export. Identification reads
one complete handshake without changing state or sending replies. The host bounds
input bytes and validates the returned kind/value. SDK Debug redacts the value.
No tenant, internal device ID or authorization field exists in this handoff.

Resolve the claim against registered devices under a host-selected channel policy
before accepting device-scoped traffic. Serial or IMEI possession is not proof
of identity. The sample's nonempty state check is protocol sequencing only.
Identification is not automatically called by ordinary session turns.

Each invocation receives a fresh component instance. Durable continuity must use
explicit session-state bytes, never globals, filesystem, clocks or network calls.
Framing is side-effect-free. State changes and decoded output pass existing host
gates atomically; transport and persistence remain host responsibilities.

## Conformance and extension

The suite covers legacy admission compatibility, artifact limits, malformed
components, bounded identity input, invalid handshake, fragmented/coalesced frames,
state continuity, deterministic replay, processing rejection and reply gates.
Existing sandbox tests cover import denial and fuel/memory/deadline failures.
A successful test does not register, activate or authenticate a device.

Copy the example's structure for a custom protocol; implement generated Guest
traits rather than manually encoding canonical ABI layouts. Keep shared WIT
changes versioned and validate old components before changing a contract.
