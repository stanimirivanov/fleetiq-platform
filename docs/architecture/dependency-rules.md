# Dependency rules

## TL;DR

- Run `cargo xtask architecture` after changing a Cargo manifest.
- Every workspace package and direct internal dependency is explicit policy.
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
| `fleetiq-xtask` | Tooling | Repository checks that do not enter production binaries |

These roles describe the current workspace. Capability crates own their
contracts and use cases directly while one backend process composes them.

## Enforced policy

The exact policy in `crates/xtask/src/architecture/policy.rs` covers every
direct dependency between workspace members. The checker reads Cargo's
normalized metadata and distinguishes normal, development, and build edges. It
rejects:

- a new workspace package without a role and dependency policy;
- a policy entry whose package was removed;
- a direct internal dependency absent from the allowlist;
- a stale allowance after a dependency is removed; and
- a cycle in the production dependency graph.

Optional and target-specific workspace dependencies are included even when they
are inactive on the current host. Renamed dependencies are identified by their
workspace package path. Registry dependencies and nonmember path dependencies
do not define edges in this graph; review them under the dependency criteria in
the engineering standards.

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
3. update the exact policy in the same pull request;
4. record third-party features, license, maintenance and security status,
   alternatives, and exit path when applicable; and
5. run `cargo xtask architecture` and the complete quality gate.

A temporary exception requires an accepted decision, a concrete removal task,
and a diagnostic that remains visible on successful checks. An exception is not
a general permission for other dependencies between the same roles.

## Related decisions

- [Capability dependency enforcement](../adr/0066-enforce-capability-dependency-policy.md)
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
