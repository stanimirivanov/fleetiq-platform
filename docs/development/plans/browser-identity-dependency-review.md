# Browser identity dependency review

## TL;DR

The selected OIDC implementation is openidconnect 4.0.1. Its transitive rsa
0.9.10 dependency triggers RUSTSEC-2023-0071; no patched release is available.
The FleetIQ adapter verifies provider signatures using public keys and does
not use RSA private keys, signing, or decryption. The project owner explicitly
approved the narrow exception below on 9 October 2026. All other advisory checks remain enforced.

## Exposure assessment

The [RustSec advisory](https://rustsec.org/advisories/RUSTSEC-2023-0071.html)
describes private-key recovery from observable timing of private RSA operations.
The dependency path is fleetiq-server -> openidconnect 4.0.1 -> rsa 0.9.10.
FleetIQ uses CoreClient discovery, authorization-code exchange, and
IdToken::claims signature verification. The only client credential is a
confidential OIDC secret; no RSA client assertion is created. The adapter loads
provider public JWKS and does not accept a private key, decrypt a message, or
call the library's signing API. Crypto tests use EdDSA fixture signing, not RSA.

An alternative library must preserve equivalent issuer/audience/nonce/expiry
and signature checks and provider interoperability. Replacing protocol
verification with ad-hoc JWT validation is not a suitable shortcut. This review
records the approved dependency and usage; it does not claim that rsa itself
is patched.

## Approved exact exception

The pinned `cargo-deny` 0.20.2 does not yet implement the upstream
`advisories.ignore.allow` field. The approved scope is enforced with supported
configuration and an exact lockfile sensor; the tool pin is unchanged.

```toml
[advisories]
ignore = [
    { id = "RUSTSEC-2023-0071", reason = "OIDC adapter performs public-key verification only; no RSA private-key operations or keys" },
]

[bans]
deny = [
    { crate = "rsa", wrappers = ["openidconnect"], reason = "Only the reviewed OIDC verifier may depend on RSA" },
    { crate = "openidconnect:<4.0.1", reason = "OIDC versions outside 4.0.1 need a new RSA exposure review" },
    { crate = "openidconnect:>4.0.1", reason = "OIDC versions outside 4.0.1 need a new RSA exposure review" },
]
```

The wrapper rule checks every direct RSA consumer. The version bans reject
other stable OIDC releases. `cargo xtask architecture` additionally parses the
lockfile with the maintained TOML library and requires exactly the reviewed
crates.io packages `openidconnect` 4.0.1 and `rsa` 0.9.10, with only the reviewed
direct edge. It rejects prereleases, duplicate versions, changed package
sources, additional RSA consumers, and malformed lockfiles. Both architecture
and supply-chain checks are required before merge and run in CI.

Browser identity maintainers own this exception. Re-review before dependency
changes or private-key operations; remove the ignore, associated bans, and
lockfile guard together when upstream is patched or the path is replaced. The
approval covers only the documented verification-only path, not private-key
use, another consumer, or an unreviewed dependency upgrade. All other advisory
checks remain enabled.

The supported [wrapper policy](https://embarkstudios.github.io/cargo-deny/checks/bans/cfg.html#wrappers)
and upstream [changelog](https://github.com/EmbarkStudios/cargo-deny/blob/main/CHANGELOG.md)
explain the tool-version constraint. Focused harness fixtures prove the exact
lockfile boundary; temporary dependency-policy mutations verify rejection of
another RSA consumer and another stable OIDC version.

## HTTPS verification licenses

The provider client uses rustls TLS and public-root verification. Exact
permissive license exceptions cover ring 0.17.14, rustls-webpki 0.103.15, and
untrusted 0.9.0 (ISC), and webpki-roots 1.0.9 (CDLA-Permissive-2.0 public-root
data). The global license allowlist is unchanged. Browser identity maintainers
own review when these versions change; remove entries when dependencies are
replaced. These license entries do not suppress any vulnerability advisory.

## Harness parser

The architecture sensor uses `toml` 0.9.12 with only `parse`, `serde`, and `std`
features. The parser is MIT OR Apache-2.0 licensed, requires Rust 1.76 or newer,
and runs only in repository tooling. Reusing it avoids a custom Cargo lockfile
parser and introduces no production authentication dependency. Remove it if
the exception guard is replaced and no other harness check consumes it.
