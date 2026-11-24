# Platform verification harness

## TL;DR

Use `cargo xtask verify --list` to discover checks and the
[verification guide](../docs/development/verification.md) to run them.
The [registry](manifest.yaml) pairs each executable catalogue ID with its
guidance and remediation. The existing repository policy check validates
the registry.

## Registry

The [executable catalogue](../crates/xtask/src/verification/catalogue.rs)
owns commands, prerequisites, and group membership. The registry stores
metadata in the JSON subset of YAML so xtask can read it with its existing
parser. The platform owns all entries in this manifest.

- Register every catalogue ID exactly once in `sensors`.
- Set `paired_guide` to a registered guide ID and `remediation` to a
  repository-relative Markdown path with an existing heading anchor.
- Add each guide to `guides` once and give it a `documentation` reference
  in the same format. Each registered guide must have a sensor.
- Set `blocking` to `true` for these checks. A selected check must pass
  for its verification group to pass. Group membership comes from the catalogue.

Run `cargo xtask repository` after changing an ID, a guide, or its heading.
Release operations are tracked separately in
[platform issue #464](https://github.com/stanimirivanov/fleetiq-platform/issues/464).
