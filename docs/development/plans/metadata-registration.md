# Asset and communication-device registration

Status: Implemented and locally verified; awaiting PR review, 16 October 2026.

## TL;DR

Deliver original plan PR 9 in M02 - Product Foundation, issue #298.
Expose two create-only commands and document the existing device read while
preserving all previous operation/component contracts.

## Scope and architecture

Metadata owns complete registration values, semantic equality, consumed exact
tenant authorization and a narrow registration port. PostgreSQL reuses existing
strict set-based aggregate SQL through an executor parameter, adding one atomic
immutable first-actor/time record. HTTP adapters own bounded closed DTOs, current
authentication and browser mutation guards. No dependency edges or packages change.

Migration 0029 adds evidence without rewriting/backfilling existing objects.
Composite references preserve tenant ownership; a generated device-only reference
also prevents device evidence without capability. Evidence rejects UPDATE, DELETE
and TRUNCATE. Signed Unix milliseconds match the existing explicit time boundary.

POST the asset collection, then optionally POST the existing asset's device
resource. Return 201 for creation, 200 plus original evidence for exact content,
409 for collisions/unattributed legacy data and 404 for missing same-tenant
references. Identifier order is not semantic; endpoint order is. Unknown or
duplicate nested fields, forged context, more than 100 children, invalid values
and bodies larger than 32 KiB are rejected.

Unique parent collisions wait for a complete winner. Roll back the failed
transaction before fresh content/evidence reads. External identity collisions
never become retry success. A lost commit response remains ambiguous; repeat
the identical key/content. No rename, replacement, deletion, protocol activation,
bindings, UI editor or historical-attribution backfill is introduced.

## Verification

Application tests prove consumed scope and actor attribution. Real PostgreSQL
tests prove complete-content retries, concurrent same/different content, external
namespace collisions, empty collections, original receipts, missing/foreign
references, audit failure rollback and immutable evidence. Composed HTTP/browser
tests prove existing reads, exact grants, fresh revocation/suspension, Origin/CSRF,
closed bounded DTOs, safe errors and method discovery. Contract tests validate
examples and negative schemas; compare preceding operations/components structurally.

Run authoritative acceptance and OpenAPI groups against PostgreSQL 18.6 with all
29 migrations and online SQLx metadata. Apply the full chain to an empty database
and upgrade a database with the previous chain and pre-existing metadata.

See [ADR 0107](../../adr/0107-register-assets-and-communication-devices.md)
(Proposed for human review) and the [operator procedure](../../operations/metadata-registration.md).
The final local evidence is recorded below.

## Verification evidence

Trusted base: `0a98578cd9cb24986779ac7645761e77ae496af4` (`origin/main`).

- **Passed:** `cargo xtask verify --group acceptance --base origin/main`,
  all thirteen checks: repository, architecture, migration immutability, secrets,
  real scanner fixtures, supply chain, dependency use, formatting, online SQLx,
  strict Clippy, runtime tests, doctests and Rust documentation. Evidence:
  `target/verification/acceptance-7376-1792161631796222600.json`
  (`complete_pass: true`).
- **Passed:** 868 runtime tests across 63 binaries, zero skipped, including
  24 new tests; all four compile-fail doctests passed.
- **Passed:** Authoritative OpenAPI verification, including 64 sensor fixtures,
  pinned linting and exact bundle freshness. Evidence:
  `target/verification/openapi-97812-1792151946760962800.json`.
- **Passed:** Structural comparison with the trusted bundle preserves all
  35 preceding paths, 40 path entries and 184 components exactly. Baseline.11
  contains 36 paths and 197 components, with three additive operations.
- **Passed:** PostgreSQL 18.6 empty-database application of all 29 migrations and
  upgrade from migration 0028 with existing assets, identifiers and endpoints.
  The upgrade retains content without inventing registration evidence.
- **Passed:** Final real-database tests used an isolated disposable PostgreSQL
  18.6 instance with `fsync`, `synchronous_commit` and `full_page_writes`
  enabled. Existing development databases and unrelated containers were preserved.
- **Failed:** None in the final acceptance or OpenAPI run. Earlier attempts
  exposed an immutable-definition fixture, an outdated exact operation inventory,
  a strict test-function length lint and local linking disk exhaustion; each was
  corrected before the final complete run. Failed attempts were not accepted.
- **Not run:** Hosted GitHub CI before PR creation.

Evidence files are ignored local execution metadata, not committed source.
Only verification-status documentation changes follow the complete acceptance
run; repository, formatting and diff checks are repeated before committing.
ADR 0107 remains Proposed for human review. After this slice merges, continue
with original plan PR 10: relationship and signal-binding revision commands.
