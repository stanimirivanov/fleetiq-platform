# Ingestion diagnostics and bounded-load qualification

## TL;DR

Read `GET /api/v1/tenants/{tenant_id}/assets/{asset_id}/ingestion-diagnostics`
with current tenant `protocol.manage`. It returns safe persisted evidence and a
bounded observed-source sample. Worker and connection liveness are unknown.
The measured TCP fixture records transport/storage results separately from
protocol execution; it does not establish a production capacity SLA.

## Operator procedure

1. Sign in using the configured HTTP authentication mode; select the tenant
   and a registered communication-device asset. Ordinary metadata grants do not
   grant protocol diagnostics authority.
2. Read the diagnostic endpoint. `404` means no such communication device in
   this authorized tenant. `200` with empty `frames` means no accepted messages
   were observed, not that the worker or device failed.
3. Inspect exact decoder package/version per frame. Check current registration/
   admission through the [package API](protocol-package-admission.md) and current
   channel configuration through [protocol routes](protocol-route-commissioning.md).
   Historical package provenance does not establish current eligibility.
4. Interpret processing evidence using the table below. Compare `received_at_ms`
   with `observed_at_ms` as receipt age only; do not label it processing duration.
5. Use discovered package/endpoint/signal tuples for source selection. Check all
   `coverage` flags; absence from this sample does not disprove device support.

| Evidence | Meaning | Next investigation |
|:--|:--|:--|
| `accepted` | Raw receipt, no completion/replay-work evidence | Check the accepting workflow; legacy accepted frames may not have replay work |
| `queued` | Replay work exists but was never claimed | Check recovery-worker configuration and compatibility; queue state alone does not prove a process failure |
| `leased` | Database lease was unexpired at the snapshot | Allow bounded processing; the claimant might nevertheless have stopped |
| `expired` | Lease expiry is at or before the database snapshot clock | Check [recovery](protocol-replay.md); expiry alone is not terminal poison |
| `poisoned` | Scheduler recorded terminal rejection or exhaustion | Investigate the exact package/release and recovery evidence before intervention |
| `decoded` | Exact completion marker exists without outstanding replay work | Zero observations is a valid successful result |

A decoded marker does not erase pending or poisoned replay evidence: legacy
result persistence can precede checkpoint processing. Diagnostic processing
state prioritizes outstanding/poisoned replay; `observation_count` independently
reports the existing decoded marker. It never converts that combination into
successful worker completion.

All success/error responses are `no-store`. Errors redact database details.
Neither worker heartbeat nor socket tracking is collected by this API;
`worker_liveness` and `connection_liveness` are always `not_observed`.
No traffic cannot distinguish idle, disconnected and never-connected devices.
No endpoint exposes raw payload, opaque state or telemetry values.

## Coverage and database cost

The read-only repeatable transaction binds device existence, database clock,
receipt selection and source discovery to one snapshot. Each statement has a
two-second timeout. A failure returns no partial diagnostic result.

- Inspect 64 receipts plus one sentinel through the ordered tenant/device index;
  equal receipt times use descending message identity. The sentinel is not
  included in joins for source discovery.
- Join result/lease evidence by exact tenant/message/package keys. Inspect at
  most 64 decoded positions per sampled frame before DISTINCT: at most 4,096
  source rows, regardless of historical receipt count.
- Return at most 128 distinct tuples in package/version/endpoint/signal order.
- `frames_truncated`: older receipts exist. `observations_truncated`: some
  completed sampled frame has more than 64 positions. `sources_truncated`: more
  than 128 distinct tuples exist in the inspected prefix. False flags do not
  include future, unfinished or unsampled traffic.

Migration 0034 adds the ordered index. Apply it before this binary is started;
on a large deployed log reserve a maintenance window for its transactional
writer-blocking index build. Existing receipts/results are unchanged. Rollback
means restoring the previous application with its reviewed compatible schema
plan; do not edit/remove applied migration history.

## Repeatable measured workload

Prerequisites: repository Rust/nextest toolchain, fully migrated PostgreSQL 18.6,
and `DATABASE_URL` pointing only to a disposable test database. SQLx test creates
an isolated database. Do not run this against a customer's database. Docker is
an optional way to provide PostgreSQL; no Kubernetes or D06 environment is used.

```bash
cargo nextest run -p fleetiq-server --lib --locked --profile ci -E 'test(measured_bounded_tcp_envelope)' --success-output immediate
```

For comparable measurements, run the Linux test process and PostgreSQL on the
same host or private container network; record forwarding or virtualization
boundaries. Run a single test process on an otherwise idle host. Keep database/host CPU and
memory limits recorded alongside the report when comparing revisions. The
fixture writes synthetic aggregate JSON to ignored
`target/ingestion-measurements/tcp-<pid>.json` and prints `INGESTION_MEASUREMENT`.
Retain the JSON and command/toolchain/environment description as evidence.
The CI Rust evidence artifact retains these aggregate reports for 14 days.

The workload uses two real loopback TCP connections, each with one identity
frame followed by telemetry for a ten-second sampling window, capped at 255
telemetry frames per connection (at most 512 frames and 1,024 wire bytes).
A turn already in flight finishes under the existing production deadlines.
Each connection must complete at least one telemetry frame. A third same-peer
connection must close while both admitted connections remain open. PostgreSQL
must contain exactly one raw receipt and decoded completion marker for every
sent frame, with zero pending replay work. Diagnostics must return the newest
min(64, sent frames) completed frames with truthful history truncation.
ACK measurements cover actual telemetry frames; ACK follows raw durability,
not decoded completion. Elapsed time includes both handshakes and every decoded
completion; its timer stops before the diagnostic query. The report records
actual counts and the sampling window/cap, rather than requiring a portable
minimum frame rate. Existing production 300-second connection lifetime and
read/write/turn limits remain unchanged. The overall fixture retains that
lifetime as its outer completion bound.
The reviewed Nextest override reserves all eight runner slots for this fixture,
so it runs without competing suite work. This does not reserve the host against
other projects. Raw cargo-test invocations must select this fixture alone.

Report throughput, ACK p50/p95/p99, Linux process RSS before/after and lifetime
high-water RSS. Memory values are null on unsupported hosts. These observe the
Rust test process, not PostgreSQL or a production container, and include test
runtime overhead. They cannot prove a memory ceiling or absence of leaks.

This fixture uses the production transport, admission/session policy and real
storage adapters with a deterministic two-byte decoder. It excludes vendor wire
costs, isolated child startup and Wasm execution. Run existing source-built
Teltonika, process supervision, slow-peer/deadline, global eight-connection and
replay recovery fixtures separately through the complete Rust gate. Those tests
and image qualification remain required; this measurement does not replace them.

## Release interpretation

A functional failure blocks the PR. Throughput/RSS observations are recorded,
not compared to invented portable thresholds. Calibrate hardware-specific
performance budgets only after realistic payload/device rates and deployment
resources are known. Full-profile capacity qualification and D06 remain deferred.

See [ADR 0117](../adr/0117-publish-bounded-ingestion-evidence.md).
