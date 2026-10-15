# FleetIQ operations

## TL;DR

This section contains procedures for explicit operator actions. Development
setup remains in contributor documentation; production deployment, backup,
restore, and incident runbooks will be added when those capabilities exist.

## Available procedures

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
