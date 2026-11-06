# ADR 0142: Encode exact archive segments

## Status

Proposed, 6 November 2026.

## Context

M06 needs an exact portable evidence segment before object publication, restore or
retention can consume it. Archive input is untrusted; byte limits alone do not
prevent decompression, dictionary or nested-schema allocation amplification.

## Decision

The new fleetiq-archive adapter uses Apache-2.0 parquet 60.1.0 with default features
disabled, plus the already reviewed bytes and SHA-256 primitives. No Arrow engine,
cloud client, native compression, runtime dependency into history/domain or object
store is introduced. Upstream references: [Parquet Rust](https://arrow.apache.org/rust/parquet/index.html)
and [source/license/features](https://github.com/apache/arrow-rs/tree/main/parquet).

The all-target dependency graph requires an exact tiny-keccak 2.0.2 CC0 license
entry for Parquet's WASM compile-time hashing path. The owner, wrapper restriction,
license review and removal condition are recorded in
[dependency management](../development/dependency-management.md#m06-archive-dependency-review).
No advisory or global license allowance is changed.

Version one is one tenant, one flat row group and 23 required UTF-8 string columns.
The wire module names the exact column order. Fields retain tenant, immutable
interpretation and decoder identity, full source/target, unit/quality, optional
device time, selected event time and first-append cutoffs. Signed times/integers,
revisions, definition versions and arbitrary-precision decimals are strings.
Empty optional unit/device-time fields mean absent; text may itself be empty.
Validating domain constructors reconstruct every row; canonical re-encoding of
fields rejects alternative numeric spellings. Duplicate interpretation identities
and mixed tenants are rejected. Input order is retained without promising a
complete event-time window or replaying semantic selection.

Limits are 256 records, 8192 UTF-8 bytes per field, 1 MiB aggregate field bytes,
2 MiB file and 32 KiB footer. Oversized domain text and decimal exponents are
checked before cloning/formatting. The writer uses uncompressed Parquet V1/PLAIN
without dictionaries, statistics or indexes. This is a safety baseline, not a
compression or production capacity claim. Compression requires another reviewed
profile and representative resource evidence.

The reader checks file/footer bounds before native metadata parsing and supplies
the fixed flat schema to avoid reconstructing attacker-controlled schema trees.
It validates one row group, bounded counts, uncompressed inline columns and exact
contiguous byte ranges. A small canonical V1/PLAIN page fence checks the emitted
header layout and each length-delimited UTF-8 field before reconstruction. It
never invokes the generic native page decoder. Dictionary/V2/compressed pages,
external column paths, oversized counts/lengths and unsupported version metadata
are rejected. This is deliberately not a general-purpose Parquet importer.

Metadata carries property-history-v1 and SHA-256 over versioned, length-delimited
logical fields in row order. The digest detects accidental evidence corruption;
it is not authenticity or whole-file integrity. Harmless metadata differences may
leave logical evidence unchanged. Later object manifests must validate the whole
object hash before publication/restore and retain the original provenance anchors.
Errors expose a fixed category without row values or underlying parser diagnostics.

## Consequences

No migration, process, HTTP route, network access or hot-data deletion is added.
The codec can round-trip independent evidence but cannot establish archive coverage
or authorize reclamation. Future format changes need explicit version handling;
Parquet dependency upgrades must rerun native interoperability and profile fixtures.
The private page fence is intentionally tiny and coupled to this owned writer
profile, not an alternative general Parquet/Thrift implementation.

## Alternatives considered

- Arrow/DataFusion: unnecessary dependency and execution surface for bounded segments.
- A new JSON archive protocol: rejected in favor of interoperable Parquet output.
- Generic compressed Parquet input: deferred because its allocation surface exceeds
  the narrow initial archival contract.
- Downcast decimals/times or omit attribution: rejected because restore must preserve evidence.

## Verification

Round-trip fixtures cover all value/quality kinds, units, optional device time,
maximum revisions/versions and extreme signed times. Tests cover truncation,
mutations, unknown versions, invalid pages, limits, duplicate/scope rejection and
native Parquet-reader interoperability. The representative 256-row synthetic
fixture emits 857811 bytes against the 2097152-byte file cap. A standalone Linux
test-process run measured 13148 KiB maximum RSS and 0.04 s elapsed; compiler memory
is excluded. These are local observations, not target capacity certification.
Run cargo test -p fleetiq-archive --test codec representative_segment -- --exact --nocapture;
measure the produced test executable separately with /usr/bin/time -v. Full Rust
acceptance, architecture, license/advisory and supply-chain gates remain required.
