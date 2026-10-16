# Attributed relationship and signal-binding revision commands

Status: Implemented and locally verified; awaiting PR review, 16 October 2026.

## TL;DR

Deliver original plan PR 10 in M02 - Product Foundation, issue #300.
Expose complete revision appends/corrections with original actor attribution and
publish both existing exact-revision GETs without changing their representation.

## Scope and boundaries

Metadata owns complete revision commands, content equality and consumed exact
management authorization. PostgreSQL composes existing strict append functions
inside one caller-owned transaction with immutable attribution. HTTP owns bounded
closed DTOs, trusted clock sampling, selected authentication and mutation guards.
No dependency, crate, workspace edge or existing strict API is replaced.

The path selects identity and positive-u64 decimal revision. The body selects
complete endpoints/type or source/target/property and physical interval. Actor,
tenant and recorded time are trusted context. Revision 1 starts history; the
exact successor corrects it. Full-content identical retries compare the requested
immutable revision even after later corrections, excluding newly sampled time.
Legacy unattributed rows never receive fabricated authorship.

Migration 0030 adds exact-fact actor records with tenant-safe references and
update/delete/truncate guards. The original fact continues to own recorded time.
No existing row rewrite/backfill. Existing sequence, monotonic recorded-time,
half-open effective-time and inclusive known-time rules remain unchanged.

## Verification and delivery

Test consumed tenant scope and actor attribution, real concurrent equal/different
appends, complete old-revision retry after correction, exact reads and temporal
boundaries. Cover reference/membership failures, legacy rows, gaps, clock
regression, audit rollback/immutability and ambiguous commit errors. Composed
HTTP/browser tests cover authority freshness, Origin/CSRF, strict JSON, 32 KiB
body limit, safe no-store failures and merged method discovery.

Run complete authoritative acceptance and OpenAPI groups, regenerate online SQLx
metadata, preserve all preceding path entries/components structurally and verify
empty/upgrade migration application on PostgreSQL 18.6. Record actual final
evidence below.

See [ADR 0108](../../adr/0108-expose-attributed-metadata-revision-commands.md),
Proposed for human review, and the [operator procedure](../../operations/metadata-revisions.md).
Corrections retain the existing whole-fact semantics, without interval merging
or atomic handovers across identities.
No UI editor, graph-wide ontology engine, destructive change, protocol activation
or automatic semantic reprocessing is included. Original plan PR 11 is the
complete development installation in fleetiq-deploy.

## Verification evidence

Trusted base: `8e9074bdd16d172ebe57c703353f57328385bd39` (`origin/main`).

- **Passed:** `cargo xtask verify --group acceptance --base origin/main`,
  all thirteen checks, with `complete_pass: true`. Evidence:
  `target/verification/acceptance-106176-1792167734499318300.json`.
  Includes current repository/architecture/history/secrets/scanner policy,
  supply chain, dependency use, formatting, online SQLx, strict Clippy,
  full tests, doctests and Rust documentation.
- **Passed:** 886 runtime tests across 65 binaries, zero skipped, including
  18 new tests. Ten tests reported slow execution on this host; no timeouts,
  concurrency limits or checks were changed. All four compile-fail doctests
  and workspace Rust documentation passed.
- **Passed:** Authoritative OpenAPI verification: 64 sensor tests, pinned
  linting and byte-exact bundle freshness. Evidence:
  `target/verification/openapi-94492-1792165463955335700.json`.
- **Passed:** Structural comparison preserves all 36 preceding paths,
  43 path entries and 197 components exactly. Baseline.12 contains 38 paths,
  214 components and four additive operations.
- **Passed:** Full 30-migration chain on empty PostgreSQL 18.6; populated
  migration-0029 upgrade preserves prior relationship/binding facts and recorded
  times, with zero fabricated attribution. Final tests used an isolated
  disposable instance with fsync, synchronous_commit and full_page_writes enabled.
  Existing development containers/databases were preserved.
- **Failed:** None in final acceptance or OpenAPI. Earlier development runs
  caught dynamic test SQL, strict field-name/semicolon lints, a generic denial
  component missing published no-store metadata, and the manual seed's missing
  transaction boundary. Each was corrected without weakening checks.
- **Not run:** Hosted GitHub CI before PR creation.

Evidence files are ignored local execution metadata, not committed artifacts.
After acceptance only verification-status documentation is updated; repository,
formatting and diff checks are repeated before committing. ADR 0108 remains
Proposed for human review. Continue with original PR 11 after this slice merges.
