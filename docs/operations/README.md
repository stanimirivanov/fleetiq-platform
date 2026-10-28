# FleetIQ operations

## TL;DR

This section contains procedures for explicit operator actions. Development
setup remains in contributor documentation; production deployment, backup,
restore, and incident runbooks will be added when those capabilities exist.

## Available procedures

- [Backend image publication](image-publication.md) — qualify immutable production bytes, review provenance/inventory and manually publish by digest.

- [Runtime and migrations](runtime-and-migrations.md) — explicit migration, checked production serving/bootstrap, role privileges, readiness, OIDC CA trust and shutdown.

- [Browser operator identity](browser-identity.md) — configure provider-neutral
  OIDC, same-origin HTTPS, operator mapping, sessions, and logout.

- [Bootstrap the first platform operator](platform-bootstrap.md) — initialize
  the installation's first internal platform authority through the one-shot
  process command.
- [Protected asset queries](asset-query.md) — configure the first credential,
  provision its identity and grant, and read exact assets or a bounded tenant
  catalogue.
- [Protected communication-device query](device-query.md) — read a device
  asset's ordered endpoint declarations with the same tenant grant.
- [Protected asset-type query](asset-type-query.md) — resolve an asset's pinned
  immutable type version and ordered property references.
- [Protected property query](property-query.md) — resolve a pinned property
  version to its name, value kind, and optional canonical unit.
- [Protected relationship-type query](relationship-type-query.md) — resolve a
  pinned directed-edge type to its operator-facing name.
- [Protected relationship revision query](relationship-revision-query.md) —
  inspect one exact recorded edge and its effective interval.
- [Protected asset relationship snapshot](asset-relationship-snapshot.md) —
  page the edges involving one asset at explicit physical and recorded times.
- [Protected signal-binding revision query](signal-binding-revision-query.md)
  — inspect one exact source-to-property attribution and its effective interval.
- [Protected signal-binding snapshot](signal-binding-snapshot.md) — page the
  attributions from one decoded source at explicit physical and recorded times.
- [Protected asset-binding snapshot](asset-binding-snapshot.md) — page the
  attributions targeting one asset at explicit physical and recorded times.

- [Operator tenant discovery](operator-tenants.md) explains bounded current-access hints.

- [Actor provisioning and lifecycle administration](actor-administration.md)

- [External-identity administration](external-identity-administration.md) — attach/detach, current mappings, and immutable audit.

- [Platform-access administration](platform-access-administration.md) — exact grant/revoke, current grants, immutable audit, and safe delegation.

- [Tenant provisioning and lifecycle](tenant-administration.md) — provision initial access, activate/suspend, and inspect catalogue and lifecycle audit.

- [Tenant-access administration](tenant-access-administration.md) — exact tenant grant/revoke, current grants, audit and delegation limits.

- [Semantic definition commissioning](semantic-definitions.md) — publish immutable types/properties and discover exact versions.

- [Asset and communication-device registration](metadata-registration.md) — atomic commissioning, original attribution, collisions and safe retries.

- [Relationship and signal-binding revisions](metadata-revisions.md) — explicit corrections, original attribution and safe exact retries.

Protocol package operators: [storage and isolated admission](protocol-package-admission.md).

- [Protocol route commissioning](protocol-route-commissioning.md): exact device/channel activation, retirement and host handoff validation.

- [Stateful protocol replay](protocol-replay.md) — fenced durable recovery, isolated execution, upgrade compatibility and no historical replies.

- [Bounded TCP ingestion](tcp-ingestion.md) — explicit trusted-channel activation, limits, reply stages and recovery.

- [Ingestion diagnostics and bounded-load qualification](ingestion-diagnostics.md) — safe evidence, sampled sources and repeatable transport/storage measurements.

- [Durable semantic worker](semantic-worker.md): checked startup, frozen interpretation choices, retained outcomes and recovery.

- [Property history and provenance](property-history.md): telemetry authorization, bounded windows, cursor/precision rules and recovery.

- [Bounded semantic graph](semantic-graph.md): rooted temporal exploration, traversal budgets and incompleteness.

- [Current property state and projector](current-property-state.md): atomic batches, snapshots, freshness, role grants and tenant rebuild.

- [Exact-property live updates](property-live-updates.md) — authorized SSE, coalesced resume, terminal recovery and resource bounds.
