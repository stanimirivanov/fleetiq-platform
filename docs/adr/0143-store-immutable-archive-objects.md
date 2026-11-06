# ADR 0143: Store immutable archive objects

## Status

Proposed, 6 November 2026.

## Context

Exact segments need an immutable storage boundary before manifests can publish
coverage. A successful upload alone does not prove addressable bytes, and retries
must handle a lost response without overwriting existing evidence.

## Decision

The archive adapter adopts object_store 0.14.2 (MIT/Apache-2.0, MSRV 1.85), enabling
only aws-base and ring. An owned HttpConnector reuses existing reqwest 0.12/Rustls
rather than adding reqwest 0.13 or another crypto provider. The async-trait and
http-body-util adapters implement that connector; all exact dependencies have
archive ownership. No additional license or advisory exception is introduced.
See [object_store](https://docs.rs/object_store/0.14.2/object_store/) and
[conditional put](https://docs.rs/object_store/0.14.2/object_store/enum.PutMode.html).

S3ArchiveConfig validates operator endpoint, bucket, region and explicit key pair.
FLEETIQ_ARCHIVE_ENDPOINT, FLEETIQ_ARCHIVE_BUCKET, FLEETIQ_ARCHIVE_REGION,
FLEETIQ_ARCHIVE_ACCESS_KEY and FLEETIQ_ARCHIVE_SECRET_KEY are complete-or-absent
server inputs. Configuration alone starts no worker or network request. Debug and
errors redact all values. Provision secrets privately; do not place them in argv
or logs. Ambient AWS metadata, profile and credential discovery are not used.

HTTPS uses normal certificate verification; HTTP is restricted to literal loopback
for fixtures. Endpoint userinfo, query, fragment and non-root paths are rejected.
Bucket names are deliberately limited to 3–63 lowercase letters/digits/hyphens.
The transport enforces the fixed origin, disables redirects/proxy discovery and
bounds request and response bodies to the codec's 2 MiB file cap. Connect/request
limits are two/five seconds, and whole operations have a five-second deadline.
Automatic SDK retries are disabled; callers own explicit exact retry decisions.

ArchiveObjectRef retains validated tenant, full-object SHA-256 and exact positive
length. Keys derive only from the fixed fleetiq/archive/v1 prefix, hex-encoded
tenant bytes and digest; callers cannot supply arbitrary paths. This descriptor
is not an authorization grant or a manifest. Trusted application callers must
obtain authority before invoking storage.

Put validates the segment's tenant and profile, uses conditional create and then
reads addressable bytes back. Existing exact objects verify successfully; divergent
objects return conflict without overwrite. Reads check HEAD length, use the ETag
as a GET precondition, and verify actual length, whole-file digest and codec tenant.
This digest is distinct from the codec's logical-field checksum. Missing, invalid,
conflicting and unavailable outcomes remain closed/redacted categories. No delete,
overwrite, list, multipart or presigned-URL API is exposed. Cancellation may leave
a completed object after an unobserved upload; exact retry verifies it. Neither
upload nor descriptor establishes archive coverage or reclamation authority.

## Consequences

No migration, new worker, public route or deployment promotion is added. Configure
an already provisioned bucket and private credentials with only required prefix
permissions, no deletion, and server-side conditional-write policy where available.
Qualify each provider's precondition and durability semantics before production.
There is no private-CA override or certificate-verification bypass in this slice.
Disabling configuration stops future consumers, not persisted objects; no cleanup
or rollback deletes evidence.

## Alternatives considered

- Raw custom S3 signing: rejected; object_store owns SigV4 and conditional semantics.
- Full AWS SDK or new TLS/crypto engine: unnecessary weight for bounded PUT/HEAD/GET.
- In-memory-only qualification: rejected because it cannot prove S3 preconditions.
- Legacy MinIO binary images: upstream marks those images unmaintained; use the
  pinned Apache-2.0 RustFS 1.0.4 fixture instead.

## Verification

The mandatory real S3 fixture covers concurrent equal creates, exact retry,
whole-object reads, scope/length/digest rejection, absence, oversized storage,
provider pause/cancel/resume and divergent objects that remain unmodified.
Transport tests reject redirects and oversized chunked bodies. Configuration tests
reject partial/unsafe inputs and check redaction. The canonical Rust gate records
the real-provider prerequisite; fixture ownership and cleanup fail closed.
The synthetic fixture is not production provider, HA or image-security certification.
Full acceptance and supply-chain checks remain required before delivery.
