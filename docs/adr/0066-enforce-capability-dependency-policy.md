# ADR 0066: Enforce capability dependency policy

## Status

Accepted, 28 September 2026.

Supersedes: none. Superseded by: none.

## Context

FleetIQ documents inward dependencies and distinct domain, application, policy,
adapter, and composition responsibilities. Cargo manifests still permit a
workspace crate to add any dependency that compiles. Reviewers therefore have
to remember the intended graph while several broad crates are being separated
into capabilities.

The mismatch is already visible in semantic resolution. ADR 0003 deliberately
introduced blanket implementations over application-owned repositories, so
`fleetiq-semantics` currently depends on `fleetiq-application`. The current
architecture describes semantic resolution as inward policy that application
orchestration may eventually consume. Adding that future dependency before
removing the existing one would create a Cargo cycle.

Prose and diagrams explain intent but cannot reject drift. A repository check
must remain small, deterministic, and based on Cargo's own model of workspace
dependencies.

## Decision

Add the repository-owned `fleetiq-xtask` package under `crates/xtask` and expose
its commands through the `cargo xtask` alias. `cargo xtask architecture` loads
the locked workspace through the maintained `cargo_metadata` library and checks
an exact policy for direct dependencies between workspace packages.

The policy classifies every workspace package as domain, contract, application,
policy, adapter, composition root, or tooling. It records normal, development,
and build dependencies separately. The check fails when:

- a workspace package has no classification;
- policy remains for a package that no longer exists;
- an internal dependency is absent from the allowlist;
- an allowlisted dependency is no longer declared; or
- the allowed production graph contains a cycle.

The check resolves workspace packages by their Cargo paths, so dependency
renames do not bypass policy. It sees optional and target-specific declarations
without activating them. Registry dependencies and path dependencies outside
the workspace remain subject to dependency review, security policy, and Cargo's
locked resolution rather than the internal architecture graph.

The `fleetiq-semantics -> fleetiq-application` normal dependency is the sole
temporary exception. ADR 0003 remains authoritative for its current blanket
adapters. The command reports the exception even when the check succeeds and
rejects it as stale once the dependency disappears. A focused follow-up will
supersede that part of ADR 0003, place the semantic snapshot query with its
consumer, move adapter ownership outward, and remove the exception before
application orchestration consumes semantic policy.

CI runs the architecture command before database-backed quality checks. The
command is also part of the documented local quality gate and pull-request
evidence.

## Consequences

Cargo remains the source of dependency facts; FleetIQ owns only the
project-specific policy. Adding a package or changing an internal edge now
requires a visible policy change. Development dependencies cannot silently
become production dependencies.

The checker validates package boundaries. Module responsibility, file size,
public APIs, and third-party suitability remain engineering and review concerns.
Capability extraction remains separate pull-request-sized work.

The tooling package adds no production runtime behavior and does not change the
single-process deployment topology. Running the check compiles a small
development binary and invokes locked, dependency-free Cargo metadata.

## Alternatives considered

Keeping the dependency diagram as a review checklist was rejected because it
cannot fail CI and has already diverged from the accepted semantic dependency.

Inferring allowed edges from broad layer ranks was rejected because legitimate
policy and adapter relationships are more precise than a total layer order.
Exact package edges produce clearer review and diagnostics.

Parsing Cargo manifests directly was rejected because Cargo owns workspace
inheritance, renamed packages, target dependencies, and manifest semantics.
`cargo_metadata` exposes Cargo's normalized model.

Source-level architecture frameworks were rejected for this check because the
stable boundary being protected is the crate dependency graph. They would not
prevent Cargo-level coupling.

Using `cargo-deny` alone was rejected because its advisory, license, source, and
dependency-ban checks address supply-chain policy rather than FleetIQ's internal
capability direction. Supply-chain hardening remains separate work.
