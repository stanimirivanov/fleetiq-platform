# Bounded TCP ingestion

## TL;DR

Run the explicit Linux `ingest-tcp` process only for a trusted identifier-routing
channel. It accepts registered devices, persists frames and decoded results, and
releases replies at their declared stage. It provides no device authentication or
TLS. D06 and full-profile qualification remain deferred.

## Prerequisites

1. Apply the reviewed migrations with the separate migration command; this
   process only checks schema. See [runtime and migrations](runtime-and-migrations.md).
2. [Store and admit](protocol-package-admission.md) the exact immutable identity-v1
   component. Teltonika and custom identity-v1 packages are supported. Keep the
   admission, live and replay sibling executors from the same immutable image.
3. [Register the communication device](metadata-registration.md), its external
   identifier and exact [device/channel route](protocol-route-commissioning.md).
   The identifier kind/value must match the component's initial claim. Configure
   an identifier authority where the registered identifier requires one.
4. Restrict ingress and database privileges for this isolated channel. An IMEI or
   serial number is a public routing value, not proof of device possession.

## Configuration

Provide environment values through the deployment's private configuration;
never commit database credentials. Values are not printed by configuration errors.

| Variable | Value |
| --- | --- |
| `DATABASE_URL` | PostgreSQL connection for the reviewed migrated installation |
| `FLEETIQ_TCP_IDENTITY_POLICY` | Explicit opt-in: `registered-identifiers` |
| `FLEETIQ_TCP_TENANT_ID` | Exact active tenant |
| `FLEETIQ_TCP_CHANNEL_ID` | Exact commissioned channel |
| `FLEETIQ_TCP_PACKAGE_ID` | Exact admitted immutable package ID |
| `FLEETIQ_TCP_PACKAGE_VERSION` | Exact package version |
| `FLEETIQ_TCP_IDENTIFIER_AUTHORITY` | Optional exact external identifier authority |
| `FLEETIQ_TCP_BIND_ADDRESS` | IP and port; default `127.0.0.1:55027` |

On Linux, from an installed directory containing the fixed sibling executors:

```sh
fleetiq-server ingest-tcp
```

For local Compose, first start and commission the development database/API, set
all channel variables above, then start the optional profile:

```sh
docker compose -f deploy/local/compose.yaml --profile tcp up --build tcp-ingestion
```

The service binds container `0.0.0.0:55027` and publishes only host loopback
`55027`; `FLEETIQ_TCP_PORT` changes the host port. The optional authority must be
passed explicitly when needed, for example with a local Compose override. The
image's HTTP healthcheck is disabled for this process mode. Missing configuration,
unsupported OS, missing executor, schema mismatch or ineligible bootstrap fails
closed before a listener is opened. There is no migration-on-start behavior.

## Runtime limits

| Resource | Limit |
| --- | --- |
| Active sockets / sockets per peer IP | 8 / 2 |
| Isolated child processes | 1; other bounded connections wait |
| Unread input / lifetime received input | 64 KiB / 1 MiB |
| Committed frames / framing polls | 256 / 512 per connection |
| Read idle / reply write / complete turn | 15 s / 2 s / 25 s |
| Connection lifetime / child deadline | 300 s / 15 s |
| State / decoded outputs | 16 KiB / 1200 per frame |
| Reply count / per reply / total | 16 / 4 KiB / 16 KiB per frame |

The existing child CPU, address-space, fuel and guest-memory ceilings apply.
These are functional safety bounds, not a measured production throughput claim.
Idle TCP keepalives do not replace protocol bytes. Long-lived devices must be
able to reconnect within this initial conservative policy.

## Acceptance, replies and recovery

- Fragmentation and coalescing preserve complete frame prefixes and ordered turns.
  Identity is resolved only from the first processed frame under host scope.
- Current admission/route checks surround execution. A retired route, changed
  identifier or suspended tenant prevents the next turn; failures close that socket.
- The parent validates the complete output/state/reply proposal before effects.
  Processing-only replies retain their weaker contract. Raw-durability replies
  wait for confirmed atomic raw/work acceptance, and cannot overtake earlier replies.
- An ACK gated on raw acceptance proves recoverable bytes, not successful decoded
  or semantic processing. Failed checkpoint completion leaves pending work.
- A fenced exact claim commits decoded results and successor checkpoint together.
  A competing recovery worker may claim first; the live socket then closes safely.
- Any failed/uncertain storage, reply, deadline or cancellation terminates the
  session. Use the [replay worker](protocol-replay.md) to recover accepted inputs;
  recovery never sends old ACKs. Keep its exact immutable execution fingerprint.
- A reconnect gets a new random session identity. Repeated wire bytes can produce
  distinct accepted facts. No cross-connection byte-based deduplication is promised.

Send SIGTERM/SIGINT to stop admission and close active sockets. The process drains
child kill/reap before closing storage. A killed process can leave accepted pending
work; preserve that database and the exact runtime needed for recovery. Never
remove a poison row or reconstruct old input manually to force a retry.

## Verification

The Rust gate covers application ordering/failure/cancellation, real TCP with
PostgreSQL, exact-claim contention, slow peers, overload, reconnect, shutdown and
source-built Teltonika framing/state/reply stages. Linux CI additionally executes
the real live child under process limits. Image policy checks require that child
in the production runtime. Operator diagnostics and bounded-load measurements are
the next separate product slice; no hardware or deployed-production claim is made.

See [ADR 0116](../adr/0116-accept-bounded-durable-tcp-sessions.md) for ownership
and consistency boundaries.
