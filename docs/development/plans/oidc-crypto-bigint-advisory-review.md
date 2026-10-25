# OIDC crypto-bigint advisory review

Status: Owner-approved on 2026-10-25; verification-only exception implemented.

## TL;DR

- Tokio 1.53.3 and ctutils 0.4.4 are compatible security updates.
- crypto-bigint 0.7.6 cannot satisfy OIDC's pinned elliptic-curve dependency.
- The reviewed ECDSA path verifies provider signatures with public keys; it does
  not accept EC private keys or perform private-key signing/key agreement.
- The owner approved only the exact verification-only exception below; all
  other advisory findings remain blocking.

## Evidence

[RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387.html)
requires crypto-bigint >=0.7.6. Cargo's online resolver rejects that version
because elliptic-curve 0.13.8 requires ^0.5. The complete locked path is:

```text
fleetiq-server -> openidconnect 4.0.1
  -> p256 0.13.2 / p384 0.13.1
  -> ecdsa 0.16.9 / primeorder 0.13.6 / elliptic-curve 0.13.8
  -> crypto-bigint 0.5.5
```

OIDC 4.0.1 declares p256/p384 unconditionally, so disabling Cargo features does
not remove them. Its upstream core/crypto.rs constructs public VerifyingKey
values from provider JWKS coordinates and calls signature verification. FleetIQ's
browser_identity/provider.rs obtains ID-token claims through the maintained
verifier; it does not call EC signing or key-agreement APIs. Test signing is
EdDSA, outside this dependency path. This is an exposure assessment, not a claim
that crypto-bigint 0.5.5 is patched or that all uses are safe.

## Approved exact exception

The owner explicitly approved ignoring only RUSTSEC-2026-0387 while enforcing
the exact crates.io package versions and complete reverse-consumer closure above.
Only the named OIDC stack and FleetIQ server may lead to crypto-bigint. Reject
additional consumers, versions (including prereleases), alternate sources,
duplicate records, removed required edges and malformed graph data. The
existing RSA exception remains separate and unchanged; all other advisory
gates remain enforced.

`deny.toml` permits only elliptic-curve as the direct crypto-bigint wrapper.
The architecture sensor additionally checks every incoming edge to each named
package, including the local fleetiq-server root, against an exact edge list.
Both guards run in existing CI groups. The prior RSA guard remains independent.

Owner: browser identity maintainers. Re-review on every affected dependency
change, private-key use, new consumer, newly relevant exploit information, or an
upstream compatible fix. Remove the ignore and associated graph guard together
when the dependency path is patched or replaced. Approval does not cover custom
cryptography, secret-integer arithmetic or other FleetIQ consumers.

## Alternative

Migrate the OIDC adapter to a maintained implementation whose complete dependency
graph is patched, preserving issuer/audience/nonce/expiry/signature verification,
PKCE, provider interoperability, HTTPS trust and bounded errors. This requires
an architecture/security review and real provider regression tests; it must not
be replaced with ad-hoc JWT verification or an unreviewed vendored crypto patch.

## Verification status

The Tokio floor is now ~1.53.3 and Cargo.lock selects Tokio 1.53.3 and ctutils
0.4.4. Focused checks passed: 118 runtime tests and four compile-fail doctests
across access, ingestion, semantics and protocol-wasm, including both source-built
guest conformance journeys. Repository/architecture and unused-dependency checks
passed. Before approval, the advisory rescan reported only RUSTSEC-2026-0387.

After owner approval, all 232 xtask sensor tests, strict all-target xtask Clippy,
formatting, repository/architecture checks and the complete `cargo deny check`
passed (advisories, bans, licenses and sources). The regression suite exercises
valid Cargo reference forms and rejects changes throughout the reviewed graph.
This exception does not represent a patched crypto-bigint release. Full
PostgreSQL-backed workspace acceptance was not run locally because the required
database is unavailable; no full acceptance pass is claimed.
