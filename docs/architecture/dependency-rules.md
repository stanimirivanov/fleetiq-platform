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
| `fleetiq-analytics` | Policy | Calculator contracts and deterministic analytics policy |
| `fleetiq-access` | Policy | Authentication, action vocabulary, authorization, and evidence |
| `fleetiq-semantics` | Policy | Temporal semantic attribution |
| `fleetiq-protocol-host` | Policy | Trusted framing, state, reply, and processing decisions |
| `fleetiq-application` | Application | Use-case orchestration and consumed ports |
| `fleetiq-postgres` | Adapter | SQLx implementations of persistence and policy ports |
| `fleetiq-protocol-wasm` | Adapter | Wasmtime component admission and invocation |
| `fleetiq-server` | Composition root | Process configuration, inbound HTTP, and concrete wiring |
| `fleetiq-xtask` | Tooling | Repository checks that do not enter production binaries |

These roles describe the current workspace. Planned capability extraction will
replace the broad application role with narrower application packages while
retaining one deployable backend process.

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
`fleetiq-application` may consume semantic policy and re-exports the snapshot
query for existing application callers. The architecture checker rejects a
new `fleetiq-semantics -> fleetiq-application` edge. This implements the
removal required by ADR 0066; see [ADR 0069](../adr/0069-own-semantic-read-ports-in-policy.md).

## Access boundary direction

`fleetiq-access` owns identity resolution, credential-authenticator and
authorization-policy ports alongside their evidence types. It depends only on
domain values. PostgreSQL implements the resolver and exact access-policy
ports directly. Application use cases consume access evidence, and re-export
the existing names for source compatibility while callers migrate to the
capability owner. The architecture checker rejects an access-to-application
dependency. See [ADR 0070](../adr/0070-extract-access-policy-boundary.md).

Tenant use cases require action-typed evidence issued from the access-policy
decision. A read grant cannot enter a metadata write or access-administration
method, while the persisted action vocabulary remains closed. See
[ADR 0071](../adr/0071-type-tenant-authorization-evidence.md).

Platform administration likewise requires action-typed evidence issued by
the platform policy decision. Identity, grant, tenant provisioning, and tenant
lifecycle authority are distinct types; see
[ADR 0072](../adr/0072-type-platform-authorization-evidence.md).

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
- [Single backend composition root](../adr/0041-establish-single-backend-composition-root.md)
