# Dependency rules

## TL;DR

- Run `cargo xtask architecture` after changing a Cargo manifest.
- Every workspace package and direct internal dependency is explicit policy.
- Normal, development, and build dependencies are checked separately.
- The semantic resolver's application dependency is the only temporary
  exception and must disappear before application code consumes semantics.

## Crate roles

| Package | Role | Responsibility |
|:--|:--|:--|
| `fleetiq-domain` | Domain | Product vocabulary, invariants, and state transitions |
| `fleetiq-telemetry` | Contract | Protocol-neutral telemetry facts |
| `fleetiq-protocol-sdk` | Contract | Values shared by protocol packages and the trusted host |
| `fleetiq-analytics` | Policy | Calculator contracts and deterministic analytics policy |
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

## Temporary semantic exception

ADR 0003 established blanket implementations from semantic read views to the
existing application repositories. Consequently, `fleetiq-semantics` currently
depends on `fleetiq-application` even though semantic resolution is inward
policy.

The architecture command reports this exact edge on every successful run. It
does not authorize other policy-to-application dependencies. Do not add
`fleetiq-application -> fleetiq-semantics` while it exists. The follow-up
refactoring will move the snapshot query to its semantic consumer, move the
repository adapters outward, and remove the exception.

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
- [Single backend composition root](../adr/0041-establish-single-backend-composition-root.md)
