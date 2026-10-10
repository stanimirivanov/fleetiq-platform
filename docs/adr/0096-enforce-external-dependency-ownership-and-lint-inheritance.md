# ADR 0096: Enforce external dependency ownership and lint inheritance

## Status

Accepted, 10 October 2026.

Supersedes: ADR 0066 (external dependency exclusion only). Superseded by: none.

## Context

The internal capability graph has deterministic enforcement, but the checker
ignores registry dependencies and paths outside the workspace. A domain crate
can therefore introduce SQLx or a helper wrapping infrastructure without
violating the internal graph. All current members inherit workspace Rust and
Clippy lints, but removing that declaration also escapes the architecture gate.
The approved harness audit calls for closing these structural gaps first.

## Decision

Extend `cargo xtask architecture` using Cargo's normalized direct dependency
metadata. Preserve the internal package roles and exact edges from ADR 0066.
Add a separate exact external policy for each owning workspace package,
canonical dependency name, and normal, development, or build kind. Every member
has an external policy, including members with no third-party dependencies.
Reject unknown owners, duplicate or invalid rules, unapproved dependencies, and
stale allowances. A test dependency does not authorize production or build use.

Inspect declarations even when an optional feature or target is inactive.
Cargo's canonical package names defeat dependency aliases. A path defines an
internal edge only when it resolves to a selected workspace member. Reject all
other direct path dependencies; an approved registry name cannot authorize an
excluded helper or a vendored path replacement. Future local helpers must
become classified workspace members or receive a separately reviewed decision
and narrow sensor change. Registry and Git source suitability remains the
responsibility of `cargo deny check`; ownership approval does not approve a
source, version, feature set, license, or advisory exception.

Because declaration-only metadata omits resolved substitutions, also reject
nonempty root `[patch]` and `[replace]` sections and root Cargo configuration
`paths`, `[patch]`, or `[source]` replacements. Check both the current and legacy
configuration filenames, and fail on unreadable or malformed files. Cargo
metadata runs from the repository root. Parent/user configuration and ambient
execution overrides remain a trusted tool-environment concern, not a claim of
this sensor. See the [Cargo override reference](https://doc.rust-lang.org/cargo/reference/overriding-dependencies.html).

Read Cargo-selected member manifests with the maintained TOML parser and
require `[lints] workspace = true` without package-local replacement lint tables.
Fail on unreadable or malformed manifests. Cargo owns manifest validity and
workspace selection; the focused parser covers lint and source-override
declarations omitted from Cargo metadata. No new third-party dependency is needed.

Run the sensor regression suite in the early CI architecture job. Negative
fixtures cover bypass attempts and stale policy as well as valid manifests.
The exact OIDC/RSA lockfile guard and protocol fixture pin checks remain in
place. Document how to change ownership policy in the canonical dependency
rules rather than enlarging the agent entry point.

## Consequences

External dependency placement and lint adoption are deterministic gates before
database compilation. Infrastructure cannot enter inward crates merely because
it comes from a registry or an excluded helper. A dependency change now needs
an explicit ownership policy update alongside its existing dependency review.
Version-only updates retain the same ownership edge and still require the
supply-chain and behavioral gates.

This is a structural sensor, not proof of module cohesion, source-level SOLID
principles, security completeness, or runtime performance. It checks declared
direct dependencies, not every transitive implementation detail or permitted
crate feature. The exact policy is reviewable Rust data, not a blanket layer
ranking. No production code, public contract, migration, or advisory exception
changes.

## Alternatives considered

A short denylist of SQLx and HTTP crates would miss new frameworks and renamed
or indirect helpers. Exact owning-package approval exposes every new edge.

Broad layer ranks would reject legitimate value libraries and test tools while
allowing excessive dependencies within a rank. The existing exact graph remains
more precise.

Relying on Clippy alone cannot protect a member that stops inheriting the shared
lint configuration. A focused manifest sensor closes that gap.

Using regular expressions for Cargo declarations would mishandle inheritance,
renames, targets, and comments. Cargo metadata and the existing TOML parser
supply the facts without introducing a parallel manifest parser.