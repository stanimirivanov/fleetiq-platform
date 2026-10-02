# FleetIQ operations

## TL;DR

This section contains procedures for explicit operator actions. Development
setup remains in contributor documentation; production deployment, backup,
restore, and incident runbooks will be added when those capabilities exist.

## Available procedures

- [Bootstrap the first platform operator](platform-bootstrap.md) — initialize
  the installation's first internal platform authority through the one-shot
  process command.
- [Protected asset query](asset-query.md) — configure the first credential,
  provision its identity and grant, and read a tenant-owned asset.
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
