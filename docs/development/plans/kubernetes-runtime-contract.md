# Kubernetes D01: production runtime contract

## TL;DR

Issue #302; M02 - Product Foundation. This is the first implementation slice of
the [deployment-owned D01-D14 sequence](https://github.com/stanimirivanov/fleetiq-deploy/blob/main/docs/exec-plans/active/kubernetes-gitops-deployment.md).
It delivers explicit migration, checked serving/bootstrap, an exact immutable
migration inventory and mounted OIDC CA trust. It does not deploy Kubernetes.

## Scope and boundaries

The PostgreSQL adapter owns exact read-only schema compatibility. The composition
root owns mode selection, process configuration, TLS trust and resource lifetime.
Domain/application policy and published HTTP responses remain unchanged. Existing
development commands keep their behavior; production uses explicit check flags.
[ADR 0109](../../adr/0109-separate-production-serving-from-schema-migration.md)
and the [operator contract](../../operations/runtime-and-migrations.md) define
compatibility, role privileges, release fencing and failure recovery.

## Verification

- Real isolated PostgreSQL covers absent/stale/unknown/dirty/checksum history,
  denied access, a non-DDL read-only role, concurrent migration locking and failure.
- Actual process commands cover migration-only execution, repeatability and
  deterministic inventory without identity/bind/database configuration leakage.
- Runtime fixtures cover schema rejection before listener binding, readiness,
  in-flight HTTP drain, browser cleanup cancellation and pool closure.
- Generated ephemeral certificates and loopback TLS cover real discovery/JWKS
  and token transport, default rejection, additional trust, wrong host, redirects,
  LF/CRLF, mixed/invalid/missing/oversized bundles and redaction.
- Run the complete unchanged authoritative acceptance and applicable local
  Compose smoke. Offline query descriptions change only for the new ledger read.
- Image publication/nonroot packaging belongs to D02; deployed browser identity
  and full install/release recovery acceptance belong to later deployment slices.

## Dependency review

serde_json already exists in the locked server tests and now owns actual public
inventory serialization in the composition root. No handwritten JSON generator
or serializer enters PostgreSQL/domain crates.

Test-only rcgen 0.14.10 (crypto/ring/pem) generates ephemeral certificates and keys;
Tokio Rustls 0.26.6 (ring/tls12, defaults disabled) serves real HTTPS using the
existing Rustls/ring family. Tokio io-util/sync are test-only feature additions.
Their declared MSRVs are Rust 1.88 (rcgen) and 1.71 (Tokio Rustls).
Both libraries are maintained MIT/Apache-2.0 crates from crates.io; verify locked
advisories/licenses with cargo-deny. MSRV is checked on pinned Rust 1.99.1.
No new runtime crypto stack, external openssl executable, static fixture key or
secret-detector exception is required. Remove these dev edges with the fixture
if an upstream real TLS fixture replaces it. The exact ownership sensor includes
the new development edges and removes the superseded serde_json dev edge.

## Delivery status

Implementation and verification are recorded in the linked PR. No local fixture
pass attests a live Kubernetes target, customer provider or published image.
