# Teltonika TCP protocol package

## TL;DR

- Build and admit this source-built identity-world component using the pinned workspace toolchain.
- Supported: a 15-digit IMEI handshake, TCP Codec 8 and Codec 8 Extended AVL packets.
- Outputs retain device timestamps and exact unknown IO bytes; no asset semantics are inferred.
- Run native wire regressions and actual Wasm host conformance before publishing.
- This does not open a listener, activate a device, qualify hardware, or resume D06.

## Build and verify

From the repository root:

```bash
cargo test --locked -p fleetiq-teltonika
cargo clippy --locked -p fleetiq-teltonika --all-targets -- -D warnings
cargo clippy --locked -p fleetiq-teltonika --target wasm32-unknown-unknown -- -D warnings
cargo test --locked -p fleetiq-protocol-wasm --test teltonika
cargo build --locked --release --target wasm32-unknown-unknown -p fleetiq-teltonika --target-dir target/protocol-guest
cargo run --locked -p fleetiq-protocol-wasm --example componentize -- target/protocol-guest/wasm32-unknown-unknown/release/fleetiq_teltonika.wasm target/protocol-guest/teltonika.component.wasm
```

The core module is not a distributable component. Use the componentizer, then
follow [artifact admission](../../docs/operations/protocol-package-admission.md)
and [route commissioning](../../docs/operations/protocol-route-commissioning.md).
Suggested package ID/version: `teltonika.tcp` / `1`; operators register the exact
host-computed digest. These names do not automatically register a release.
The source, Cargo.lock, toolchain, target, profile and authoritative WIT are build
inputs. Byte identity across different hosts is not claimed.

## Supported wire contract

The normative reference is Teltonika's
[Data Sending Protocols](https://wiki.teltonika-gps.com/view/Teltonika_Data_Sending_Protocols).
This release supports a deliberately bounded subset of that specification.

| Boundary | Release policy |
|:--|:--|
| Identity | Exact big-endian length `000f`, followed by 15 ASCII digits; no Luhn check |
| Session state | Empty before handshake; exactly `01` after successful processing; no IMEI retained in state |
| Data | TCP preamble, length, Codec `08` or `8e`, matching nonzero record counts and zero-extended CRC-16/IBM |
| Frame | At most 1280 complete bytes; at most 16 records |
| IO | At most 64 entries per record, unique IDs; fixed 1/2/4/8-byte groups |
| Extended IO | Two-byte IDs/counts, plus nonempty variable values up to 256 bytes |
| Validation | Device milliseconds fit signed 64-bit time; priority 0..2; coordinates within geographic ranges; heading 0..360 |
| Failure | Entire packet rejected, without partial observations or ACK; codes 1 malformed, 2 limit, 3 unsupported, 4 checksum |

Framing returns only the first complete prefix; following bytes remain for the
next host turn. Invalid prefixes and oversized declared lengths fail before the
body is accumulated. Record/IO limits are checked before collection allocation.
CRC has initial value zero, reflected polynomial `a001`, and zero final XOR.
Handshakes repeated after session establishment are rejected. No negative ACK
is proposed for invalid data. A fresh connection must identify again.

## Source observations

Every output uses endpoint `main` and its record's device Unix milliseconds.
The host supplies tenant, registered device, package digest/release, receipt time,
and ordered observation identity. The package never supplies those authorities.

| Signal | Value and interpretation |
|:--|:--|
| `avl.codec`, `avl.priority`, `avl.event_io_id` | Raw integer protocol fields |
| `gps.longitude_e7`, `gps.latitude_e7` | Signed integer coordinates, scale 10^-7 degrees; no floating-point rounding |
| `gps.altitude_raw` | Exact unsigned 16-bit wire field; device-specific signedness remains uninterpreted |
| `gps.angle` | Integer, source unit `deg` |
| `gps.satellites` | Integer satellite count |
| `gps.speed` | Integer, source unit `km/h` |
| `io.<decimal AVL ID>.raw` | Lowercase hexadecimal text, preserving byte width, leading zeros, unsigned 64-bit values and variable data |

Each record emits the nine fixed signals above, then IO in wire order. Zero
satellites makes coordinate, altitude, heading and speed quality `Uncertain`;
other wire values are `Good`, which means parsed successfully, not calibrated or
authenticated. Maximum proposed output count is 16 × (9 + 64) = 1168. Configure
host gates for the admitted device/channel; stricter gates may reject a packet.
Units/scaling/signedness and business meaning for vendor IO need explicit
versioned downstream interpretation; this release does not infer CAN devices,
freezers, batteries, assets or model-specific AVL catalogues.

## Identity and ACK ownership

IMEI possession is not authentication. Identification returns only a bounded
claim. The host must resolve/revalidate an authorized registered-device route
before processing the handshake and writing its immediate `01` reply.
The library does not itself prove that host authorization happened.

The four-byte big-endian AVL record-count ACK is staged
`AfterDurableAcceptance`. It cannot be read as outbound bytes before the
existing host gate receives accepted-message evidence. Raw acceptance is not
decoded-result completion. Durable state/replay workers and a network listener
are separate planned slices; callers must not manually unwrap the proposal and
write ACKs early. Retransmission/deduplication policy remains the host's concern.

## Evidence and exclusions

`tests/fixtures` contains synthetic two-record packets for both codecs, with
negative longitude, full-width unknown IO, Extended variable IO and an uncertain
GPS record. A separate manufacturer example is a CRC interoperability oracle.
Native tests cover every split, coalescing, malformed checksummed bodies,
truncation, count mismatch, duplicates and bounds. The conformance test builds
this source with Wasm-target Clippy enforced, componentizes/admits the actual bytes and exercises the real host;
there is no precompiled fallback. The only unsafe/lint allowance is the fixed,
sensor-validated generated ABI module approved for issue #321.

Not supported: UDP, Codec 16, codecs for commands, device commands, automatic
model detection, manufacturer-specific IO interpretation, hardware compatibility
claims, listener/session-registry operation or live deployment qualification.
