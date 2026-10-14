# Rustls security patch qualification

Status: Implemented and verified.

## TL;DR

Update the locked rustls package from 0.23.45 to 0.23.46 to remove the four
RUSTSEC-2026-0346 through RUSTSEC-2026-0349 findings blocking actor administration
acceptance. The lockfile changes only the rustls version and registry checksum.
The authoritative supply-chain group passes without any policy change.
Duplicate-version diagnostics remain visible warnings.

## Required capability and ownership

Milestone: M02 - Product Foundation. Browser identity maintainers own this
qualification.

The server delegates OIDC discovery and token exchange HTTPS to openidconnect,
oauth2, reqwest, hyper-rustls, and tokio-rustls. Rustls supplies their TLS
implementation. This is an outbound transport dependency of the server
composition root; no dependency enters a domain or application capability.

The [upstream advisory](https://github.com/rustls/rustls/security/advisories/GHSA-c8m5-c952-6cm4)
describes secret disclosure when a custom client session store formats session
values with Debug. The [0.23.46 release](https://github.com/rustls/rustls/releases/tag/v%2F0.23.46)
also fixes ECH rejection handling, certificate decompression allocation, and
key-exchange hint poisoning. The previous locked graph reproduces all four
RustSec errors. Applying the patch removes them from the current advisory check;
this is not an advisory exception or an assertion that every affected feature
was enabled by FleetIQ.

## Dependency review

| Concern | Qualification |
|:--|:--|
| Required capability | TLS for the existing provider HTTPS client |
| Selected change | rustls 0.23.45 to 0.23.46; no other package or dependency edge changes |
| Features | Resolved rustls features are ring, std, and tls12; no manifest feature changes |
| MSRV | Candidate declares Rust 1.71, below the workspace's pinned Rust 1.98.1 |
| License and source | crates.io; Apache-2.0 OR ISC OR MIT, covered by the existing approved license choices |
| Maintenance | Upstream published a maintained patch release for the reported findings |
| Existing exception | openidconnect 4.0.1, rsa 0.9.10, the public-verification-only review, and its exact guards remain unchanged |
| Alternatives | An advisory exception leaves a known vulnerable package in the graph; a wider OIDC/HTTP upgrade or TLS replacement is unnecessary for this fix |
| Rollout | Build and deploy the locked executable through the ordinary release process; no configuration or schema change |
| Exit path | Continue qualified compatible patches; replacing the TLS transport requires a separate boundary and provider-compatibility review |

The upstream patch changes TLS behavior, so compilation and a clean advisory scan
alone are insufficient qualification. The complete acceptance gate exercises the
existing provider verification, HTTP/session, persistence, and harness suites.
Controlled fixtures do not qualify a customer provider or deployed TLS edge.

## Duplicate-version review

The attachment's warnings are distinct from the advisory errors. The policy
already reports transitive duplicates as warnings and rejects duplicate workspace
declarations. Keep both controls and all diagnostics unchanged.

Review used cargo tree -d --locked --depth 1 and inverse paths for rustls.
Different upstream API generations explain the reported parallel versions:

| Reported packages | Existing upstream constraint families |
|:--|:--|
| base64 | openidconnect uses 0.21; HTTP and SQLx dependencies use 0.22 |
| block-buffer, cpufeatures, crypto-common, digest, hkdf, hmac, sha2 | Older OIDC/RSA and compiler crypto paths coexist with newer SQLx and SHA-256 library APIs |
| getrandom, rand, rand_core | ring, OIDC/random-number consumers, ahash, and the server use different supported random API generations |
| hashbrown, itertools | SQLx, numeric dependencies, and Wasmtime/compiler tooling require different generations |
| syn, thiserror, thiserror-impl | Existing proc macros, OIDC/OAuth, and newer infrastructure dependencies use distinct major versions |
| windows-sys | Existing target-specific dependency paths need independent review; retain the warning rather than forcing a platform binding version |
| winnow | The selected toml crate uses winnow 0.7 while its toml_parser dependency uses 1.0 |

The lockfile diff adds no package or duplicate version. Do not downgrade crypto,
override upstream requirements, or move the separately reviewed OIDC/RSA boundary
just to reduce warnings. Reassess duplicates during their owning upstream
updates; this security patch neither adds a duplicate baseline nor suppresses
diagnostics. Cargo's tree can also repeat the same version across compilation
contexts; that is not itself evidence of another locked version.

## Verification

The authoritative supply-chain group passes with the patched lockfile:
cargo-deny reports advisories, bans, licenses, and sources as valid, and
cargo-machete finds no unused direct dependency.

On 2026-10-14, the complete online acceptance gate passed all twelve checks
against fully migrated PostgreSQL 18.6, with SQLX_OFFLINE unset:

`cargo xtask verify --group acceptance --base origin/main`

The initial default-concurrency run failed while SQLx was setting up a test
database: Windows reported socket resource exhaustion (OS error 10055), before
the test's assertions ran. The complete retry used RUST_TEST_THREADS=1 to limit
independent test setup. Every test remained enabled, including explicit
concurrency tests using tokio::join!, and the previously blocked platform-access
test passed. No test, detector, threshold, or security policy was changed.

The successful metadata report is
`target/verification/acceptance-32004-1792005315669213200.json`.
No new implementation-shaped unit test is added for upstream TLS internals:
the initial advisory failure is the regression signal, and the existing tests
plus the unchanged supply-chain detector qualify the fix.
