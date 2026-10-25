# Dependency rules

## TL;DR

- Run `cargo xtask architecture` after changing a Cargo manifest.
- Every workspace package and direct dependency has an explicit owner and kind.
- Reject nonmember path dependencies; all members inherit workspace lints.
- Normal, development, and build dependencies are checked separately.
- Semantic policy owns its read ports; PostgreSQL adapts them without a
  policy-to-application dependency.
- Access policy owns authentication, authorization, and evidence; adapters
  implement its narrow ports directly.

## Crate roles

| Package | Role | Responsibility |
|:--|:--|:--|
| `fleetiq-domain` | Domain | Product vocabulary, invariants, and state transitions |
| `fleetiq-telemetry` | Contract | Protocol-neutral telemetry facts |
| `fleetiq-protocol-sdk` | Contract | Values shared by protocol packages and the trusted host |
| `fleetiq-live` | Contract | Revisioned snapshot, delta, cursor, and resnapshot contracts |
| `fleetiq-analytics` | Policy | Calculator contracts and deterministic analytics policy |
| `fleetiq-access` | Policy | Authentication, action vocabulary, authorization, and evidence |
| `fleetiq-semantics` | Policy | Temporal semantic attribution |
| `fleetiq-protocol-host` | Policy | Trusted framing, state, reply, and processing decisions |
| `fleetiq-identity` | Application | Actor, external-identity, grant, tenant, and bootstrap use cases and consumed ports |
| `fleetiq-metadata` | Application | Asset graph use cases and consumer-owned persistence ports |
| `fleetiq-ingestion` | Application | Durable ingestion orchestration and consumer-owned persistence ports |
| `fleetiq-history` | Application | Exact durable-observation processing and semantic history append/read ports |
| `fleetiq-postgres` | Adapter | SQLx implementations of persistence and policy ports |
| `fleetiq-protocol-wasm` | Adapter | Wasmtime component admission and invocation |
| `fleetiq-server` | Composition root | Process configuration, inbound HTTP, and concrete wiring |
| `fleetiq-teltonika` | Adapter | Bounded vendor wire parser and import-free Teltonika guest |
| `fleetiq-custom-protocol-example` | Tooling | Import-free, source-built custom protocol guest and authoring example |
| `fleetiq-xtask` | Tooling | Repository checks that do not enter production binaries |

These roles describe the current workspace. Capability crates own their
contracts and use cases directly while one backend process composes them.

## Enforced policy

The [internal policy](../../crates/xtask/src/architecture/policy.rs) records exact
edges between workspace members. The [external policy](../../crates/xtask/src/architecture/external_dependencies/policy.rs)
records every member's approved direct third-party packages by canonical name
and normal, development, or build kind, including an explicit empty list when
none are needed. `cargo xtask architecture` reads Cargo's normalized metadata.
It rejects:

- a new workspace package without a role and dependency policy;
- a policy entry whose package was removed;
- a direct internal or external dependency absent from its exact allowlist;
- duplicate, invalid, or stale rules after a dependency is removed;
- any direct path dependency outside the selected workspace members;
- root manifest/config source overrides that bypass declared ownership;
- a cycle in the production internal dependency policy; and
- a member without `[lints] workspace = true`, including local replacements,
  unreadable manifests, and malformed TOML.

Optional and target-specific declarations are included even when inactive on
this host. Cargo's canonical names identify renamed external dependencies;
workspace paths identify internal packages. A registry package with the same
name as a workspace member is still external. Approval of a registry dependency
never authorizes replacing it with an excluded helper or vendored path. Add a
local helper as a classified workspace member, or first propose a separate
reviewed decision and narrow sensor change.

Declaration-only metadata does not expose resolved `[patch]` substitutions.
The sensor therefore also rejects nonempty root manifest `[patch]`/`[replace]`
and root `.cargo/config.toml` or legacy `.cargo/config` `paths`, `[patch]`, and
`[source]` replacement settings. An override requires a separately reviewed
policy and sensor change; there are no approved overrides today. These guards
cover repository-root inputs, not parent/user Cargo configuration, environment,
or command-line overrides. Run the documented gates from the repository root
with trusted tool configuration; Cargo metadata also runs from that root.

## Third-party ownership and lint inheritance

Owning a dependency means that the package's current responsibility needs it.
For example, `bigdecimal` implements domain value arithmetic; SQLx belongs to
PostgreSQL persistence and server wiring, Wasmtime to the protocol adapter, and
HTTP/OIDC libraries to the server. Inner application crates use Tokio only in
tests today. These examples explain current policy; they do not grant every
crate with the same role the same dependencies.

A development allowance does not authorize normal or build use. Multiple target
or feature declarations of the same owning package, dependency, and kind form
one ownership edge. Version and feature changes keep that edge but still need
dependency review. Ownership approval does not approve a source, version,
license, or advisory exception. `cargo deny check` owns those supply-chain
questions, and `cargo machete` checks suspected unused declarations.

Every member must inherit the root Rust and Clippy configuration:

```toml
[lints]
workspace = true
```

Package-local replacement lint tables are rejected. Narrow source-level lint
exceptions still need a recorded reason under the engineering standards. The
sensor guarantees inheritance; it does not prove that the workspace lint values
or source-level exceptions are sufficient. Cargo validates complete manifest
semantics; the maintained TOML parser reads the lint and source-override
declarations omitted from Cargo metadata. See [ADR 0096](../adr/0096-enforce-external-dependency-ownership-and-lint-inheritance.md).

The architecture check also validates the exact locked OIDC/RSA packages and
consumer edge for the approved verification-only advisory exception, including
prereleases and inactive targets. Its [dependency review](../development/plans/browser-identity-dependency-review.md)
records the approved scope and removal conditions. The supply-chain ban policy
provides an additional direct-consumer guard.

Exact dependency review supplements the directional model:

- domain and contract crates do not import application or infrastructure;
- application code owns use cases and the ports they consume;
- policy crates own deterministic semantic, analytics, and protocol decisions;
- adapters implement inward ports and translate external representations; and
- the composition root selects concrete implementations.

The checker covers crate dependencies. The engineering standards continue to
govern module responsibility, source organization, public APIs, and file-size
review.

## Semantic read-port direction

The semantic resolver owns its bitemporal snapshot query and narrow binding
and property-definition read ports. PostgreSQL implements those ports directly.
`fleetiq-metadata` consumes and re-exports the policy-owned snapshot query
for metadata callers. The architecture checker rejects policy-to-application
dependency edges. This implements the removal required by ADR 0066; see
[ADR 0069](../adr/0069-own-semantic-read-ports-in-policy.md).

## Access boundary direction

`fleetiq-access` owns identity resolution, credential-authenticator and
authorization-policy ports alongside their evidence types. It depends only on
domain values. PostgreSQL implements the resolver and exact access-policy
ports directly. Identity and metadata use cases consume access evidence from
the defining crate. The architecture checker rejects an access-to-application
dependency. See [ADR 0070](../adr/0070-extract-access-policy-boundary.md).

Tenant use cases require action-typed evidence issued from the access-policy
decision. A read grant cannot enter a metadata write or access-administration
method, while the persisted action vocabulary remains closed. See
[ADR 0071](../adr/0071-type-tenant-authorization-evidence.md).

Platform administration likewise requires action-typed evidence issued by
the platform policy decision. Identity, grant, tenant provisioning, and tenant
lifecycle authority are distinct types; see
[ADR 0072](../adr/0072-type-platform-authorization-evidence.md).

## Identity application boundary

`fleetiq-identity` owns actor, external-identity, platform and tenant grant,
tenant lifecycle, provisioning, audit, and bootstrap use cases and their
consumer-owned ports. It depends on access policy and domain values, not on
a database adapter. PostgreSQL implements its ports directly, and the server
composes bootstrap directly. See
[ADR 0073](../adr/0073-extract-identity-application-capability.md).

## Metadata application boundary

`fleetiq-metadata` owns authorized asset, definition, device, relationship,
and signal-binding operations and the repository ports those use cases consume.
PostgreSQL implements these ports directly. Semantic policy retains its own
narrow read ports for resolution; metadata re-exports its snapshot query
for metadata callers. See
[ADR 0074](../adr/0074-extract-asset-graph-application-capability.md).

## Ingestion application boundary

`fleetiq-ingestion` owns accepted-message and decoded-result append ports,
protocol-package catalogue ports, and the orchestration that preserves pending
work across failed durable writes. It consumes protocol-host policy and
protocol-neutral contracts without depending on PostgreSQL or unrelated
application capabilities. PostgreSQL and its integration tests import the
ingestion ports directly. See
[ADR 0075](../adr/0075-extract-ingestion-application-capability.md).

## Live projection contract boundary

`fleetiq-live` owns transport-independent snapshot, delta, revision-cursor,
and resnapshot types. It has no workspace dependencies. A later projection
runtime can depend on this defining crate without taking on unrelated
application capabilities. See [ADR 0076](../adr/0076-extract-live-projection-contracts.md),
which refines only the crate placement in ADR 0008.

## Direct capability imports

The flat application compatibility facade has been removed. Callers import
access evidence, identity use cases, metadata ports, ingestion orchestration,
and live contracts from their defining crates. The exact dependency policy
rejects reintroducing an unclassified broad package. See
[ADR 0078](../adr/0078-retire-application-compatibility-facade.md).

## PostgreSQL adapter organization

`fleetiq-postgres` keeps one database and migration chain, but its private
`access`, `identity`, `metadata`, and `ingestion` modules mirror the ports they
implement. The crate root preserves the public adapter API. A private action
codec owns persisted platform and tenant action names for policy, command,
audit, and catalogue paths; decoders retain the exact source-column context
on corruption. See [ADR 0077](../adr/0077-organize-postgres-adapters-by-capability.md).

## Protocol fixture toolchain

The architecture check also compares exact pinned development requirements for
`wit-parser` and `wit-component` in `fleetiq-protocol-wasm`. Their fixture
helpers pass parser values into the encoder, so different releases cannot share
those Rust types. The complete fixture suite remains the compatibility check
for the associated `wat` compiler. See the
[dependency management guide](../development/dependency-management.md).

## Changing dependencies

When a task changes a manifest:

1. identify the capability and role that require the dependency;
2. confirm the dependency points toward stable domain or policy meaning, or is
   confined to an adapter, composition root, test, or repository tool;
3. update the exact internal or external policy in the same pull request, using
   the canonical Cargo package name and precise dependency kind; remove stale
   rules when removing a dependency, and preserve workspace lint inheritance;
4. record third-party features, license, maintenance and security status,
   alternatives, and exit path when applicable; and
5. run `cargo xtask architecture` and the complete quality gate; when changing
   the sensor or its policy model, also run `cargo test -p fleetiq-xtask --locked`.

A temporary exception requires an accepted decision, an owner, a reason, a
concrete removal task and review trigger, and a diagnostic that remains visible
on successful checks. Nonmember paths have no exception mechanism today; adding
one requires a separately reviewed decision and tested sensor change. An
exception is not a general permission for other dependencies between the same
roles.

## Related decisions

- [Capability dependency enforcement](../adr/0066-enforce-capability-dependency-policy.md)
- [External ownership and lint inheritance](../adr/0096-enforce-external-dependency-ownership-and-lint-inheritance.md)
- [Explicit semantic resolution](../adr/0003-explicit-semantic-resolution.md)
- [Semantic read-port ownership](../adr/0069-own-semantic-read-ports-in-policy.md)
- [Access policy ownership](../adr/0070-extract-access-policy-boundary.md)
- [Action-typed tenant evidence](../adr/0071-type-tenant-authorization-evidence.md)
- [Action-typed platform evidence](../adr/0072-type-platform-authorization-evidence.md)
- [Identity application capability](../adr/0073-extract-identity-application-capability.md)
- [Metadata application capability](../adr/0074-extract-asset-graph-application-capability.md)
- [Ingestion application capability](../adr/0075-extract-ingestion-application-capability.md)
- [Live projection contract ownership](../adr/0076-extract-live-projection-contracts.md)
- [PostgreSQL adapter organization](../adr/0077-organize-postgres-adapters-by-capability.md)
- [Application facade retirement](../adr/0078-retire-application-compatibility-facade.md)
- [Single backend composition root](../adr/0041-establish-single-backend-composition-root.md)

## Generated guest ABI boundary

Owner-approved issue #315 changes workspace unsafe policy from forbid to deny
solely to permit wit-bindgen's generated canonical ABI exports. Handwritten Rust
remains denied. Exactly two fixed out-of-line bindings modules have the owner-approved allowance:
`examples/custom-protocol/src/component/bindings.rs` and
`packages/teltonika/src/component/bindings.rs` (issue #321);
`cargo xtask architecture` parses Rust attributes with Syn and rejects other
allow/expect/cfg_attr allowances. The generated file must contain only the
reviewed generator and export invocations, preventing handwritten unsafe from
entering the exception. No allowlist permits runtime/application unsafe code.

The owner also approved only `clippy::same_length_and_capacity` on those generated
modules: canonical ABI allocations have capacity equal to length. The sensor
rejects extra allowances there; handwritten code keeps the full Clippy policy.

Protocol artifact admission adds ingestion-to-access evidence and pure SHA-256
validation, server-to-ingestion/telemetry composition and Linux-only rlimit in
the Wasm adapter worker. The server does not link Wasmtime; it supervises the
reviewed sibling executable. See [ADR 0112](../adr/0112-store-and-isolate-protocol-admission.md).

Protocol routing promotes postgres-to-protocol-sdk from development to normal
ownership: its ingestion port consumes the bounded, redacted device identity
claim. No Wasmtime dependency enters server or postgres; no new library is added.
