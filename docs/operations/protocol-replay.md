# Stateful protocol replay worker

## TL;DR

Run the packaged Linux `fleetiq-server replay-worker` against an already migrated
database. It recovers exact accepted frames without opening a listener or sending
replies. It is not a TCP ingestion service or D06 deployment qualification.

## Prerequisites

- The exact immutable backend image, including its fixed sibling
  `/usr/local/bin/fleetiq-protocol-decode` executor.
- PostgreSQL with the complete embedded migration chain through 0033. Apply
  migrations with the separate migration role; the worker checks only.
- A private `DATABASE_URL`, supplied by the deployment secret mechanism.
- Previously admitted exact protocol artifacts and host-owned accepted/replay
  input. This PR provides the repository boundary; there is no public enqueue API.
- Deployment identity, network policy and filesystem isolation appropriate for
  processing untrusted bytes. No guest imports or socket authority are granted.

## Start and stop

Run the existing image with command `replay-worker`. Do not provide inline secrets
or arbitrary executor paths. The worker uses at most two database connections and
one child at a time. Multiple workers use database fencing and skip locked work.
The image HTTP healthcheck applies to serving, not this command. Disable that
healthcheck for worker containers and do not reuse HTTP deployment probes. This
slice supplies process lifecycle and durable lease evidence, not a worker health
endpoint. Use a non-DDL role with the catalogue reads and ingestion/replay writes
required by the adapters.

SIGINT/SIGTERM stops claiming, cancels in-flight work and drains child termination
before closing the pool. The pending lease remains reclaimable after expiry.
A missing executor, incompatible schema or storage failure exits with a categorized
redacted error. A supervisor may restart the same immutable image.

## Durable ordering

1. The trusted producer supplies tenant, registered device, exact admitted
   release/world, bounded complete frame, opaque session key and pre-state.
2. Enqueue commits raw acceptance and immutable work together. Exact retries
   succeed; conflicting bytes/context or a missing predecessor do not.
3. Claim records a database-clock 30-second lease and increasing generation.
4. The child reconstructs the stored pre-state and executes that exact frame.
5. Completion commits all observations (or an empty marker), next-state receipt
   and checkpoint together. Only then can the next session turn enqueue.

A lease checked under the work/session locks owns the completion transaction.
Another claim cannot pass the locked item before commit/rollback. Older or expired
generations cannot complete. Retrying an already committed completion requires the
same complete output and successor state; it never rewinds a later checkpoint.

## Limits and failures

| Boundary | Policy version 1 |
|:--|:--|
| Frame / state | 64 KiB / 16 KiB |
| Outputs / replies proposed by guest | 1200 / 16 |
| Reply bytes each / total | 4096 / 16384; always discarded in recovery |
| Guest fuel per framing/process invocation | 50 million |
| Guest deadline / linear memory / table elements | 5 seconds / 16 MiB / 1000 |
| Guest stack | 64 KiB |
| Replay linear-memory reservation / guard | 16 MiB / 64 KiB; explicit bounds checks |
| Child wall / CPU / address-space ceiling | 15 seconds / 10 seconds / 2 GiB |
| Request / response encoded bytes | 48 MiB / 2 MiB |
| Claims | 30 seconds, at most three attempts |

Rejected component output or framing poisons the work without advancing state.
Transient executor failures leave a pending lease; after the third expired attempt,
compatible workers quarantine it as poisoned. The session cannot skip a poisoned
turn. Do not edit rows, drop guards, reset generations or delete evidence to retry.
A separately reviewed recovery operation is required.

Inspection should use bounded queries selecting identifiers, sequence, generation,
status and lease times. Do not routinely select payload, pre_state, state or
next_state; those contain sensitive protocol bytes. No diagnostics endpoint or
retention automation is supplied yet.

## Upgrades and compatibility

The session execution fingerprint is SHA-256 of the versioned replay policy tag
and exact Cargo.lock bytes embedded in the worker/executor. Both processes verify
it; claims and exhausted-lease cleanup leave other fingerprints untouched.

Drain sessions using their original immutable image before changing the lock or
replay policy, or retain that image for its sessions. A changed dependency lock,
even for unrelated dependencies, conservatively creates a different execution
identity. Never rewrite a stored fingerprint to force compatibility.
An old image also requires its exact embedded schema ledger. If an upgrade adds
migrations, drain with the old image before applying them; retaining an image
does not authorize running it against a newer schema or editing the ledger. Source changes
to execution interpretation/limits require a policy-tag bump and explicit review.

## Verification and exclusions

Real PostgreSQL tests cover claim contention, expiry, duplicate completion,
historical receipt retries, immutable tenant/provenance and rollback of nonempty
output at a failed checkpoint. Source-built Teltonika tests carry handshake state
into AVL replay and ensure the process envelope cannot encode replies. Linux CI
additionally exercises the resource-limited child process boundary.

Socket receipt, on-wire ACKs, cross-connection device retransmission, scheduling
fairness, hardware qualification, production capacity and D06 remain separate.
