# External-identity administration HTTP slice

Status: Implemented; full acceptance is blocked by existing Wasmtime advisories.

## TL;DR

Deliver agreed plan PR 5 in M02 - Product Foundation: explicit attach/detach,
actor-scoped current mappings, and bounded immutable audit review. Preserve
exact principal identity, final mappings, first-receipt retries, and redaction.

## Scope and boundaries

- Compose the existing identity services and PostgreSQL adapters through three
  configured authenticated HTTP routes.
- Reuse listener-selected browser mutation protection and exact ManageIdentity
  evidence; keep use cases and SQL out of transport handlers.
- Add explicit delivery receipt policy without weakening immutable commands.
- Publish modular baseline.6 OpenAPI, examples, contract tests, and operations.
- No new dependencies, SQL statements, migrations, provider actions, grants, or UI.

## Acceptance and qualification

Exercise concurrent deliveries with different host receipt times, strict timestamp
collisions, changed-content collisions, exact case/space/Unicode identity handling,
final-mapping protection, implicit-rebind rejection, atomic audit rollback,
deterministic bounded pages, detached cursor continuation, authentication precedence,
wrong-action denial, browser CSRF, and loss of a detached-login session.

Run focused identity, PostgreSQL, HTTP, browser, and contract tests; repository and
architecture sensors; the OpenAPI group; and complete online Rust acceptance.
The host's previous socket exhaustion warrants bounded harness concurrency; tests
that explicitly race commands remain enabled.

## Recovery and limitations

See [ADR 0101](../../adr/0101-expose-external-identity-administration-over-http.md)
and the [operating procedure](../../operations/external-identity-administration.md).
The adapter cannot establish provider account usability or verify edge log
configuration. Catalogue query cursors contain exact subjects and must not appear
in routine logs. Rollback retains mappings and immutable history.

## Acceptance blocker

The current advisory database reports RUSTSEC-2026-0353 through
RUSTSEC-2026-0364 for the unchanged Wasmtime 49.0.2 lockfile entry. Cargo-deny
reports 49.0.3 as the compatible patched line; the
[upstream exception-tag advisory](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-p79w-pv2r-29qp)
also identifies that fixed release. Cargo.lock and all Cargo manifests are
unchanged from the fetched main branch.

Qualify the runtime patch separately, including Wasm resource/admission tests,
then rerun full acceptance before merge. This HTTP slice changes no dependency,
security exception, or advisory policy. A failed advisory check remains failed;
passing focused or Rust checks does not establish full acceptance.

## Verification evidence

Final qualification on 2026-10-14 used fully migrated PostgreSQL 18.6,
SQLX_OFFLINE unset, and process-local RUST_TEST_THREADS=2. Explicit concurrent
command tests remained enabled; no check, test, or threshold was suppressed.

- `cargo xtask verify --group openapi` passed all 64 sensor tests, lint, and
  committed bundle freshness. Its report is
  `target/verification/openapi-28060-1792011757362449000.json`.
- `cargo xtask verify --group acceptance --base origin/main` finished all twelve
  checks: eleven passed and only cargo-deny failed on the existing Wasmtime
  findings above. Its report is
  `target/verification/acceptance-36048-1792013990258666600.json`.
- Workspace tests and doctests passed, including all 103 server tests, both new
  identity contract tests, the six global HTTP contract tests, and all 209 harness
  regression tests. SQLx metadata, workspace Clippy, formatting, and rustdoc passed.
- Existing path items and components were compared with fetched main: all 17
  previous path contracts and every previous component were preserved except the
  selected-actor parameter description, which now covers all administration.

The global contract guard now explicitly inventories all 20 reviewed paths and
their methods/operation IDs instead of adding an unchecked numeric allowance.
Focused application, PostgreSQL, HTTP, and browser-session tests also passed.
GitHub CI and deployed provider/edge qualification were not run. Full acceptance
remains blocked until the separate runtime dependency qualification is complete.
