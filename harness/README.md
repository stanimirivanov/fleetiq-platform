# Platform verification harness

## TL;DR

Use `cargo xtask verify --list` to discover checks and the
[verification guide](../docs/development/verification.md) to run them.
The [registry](manifest.yaml) pairs each executable catalogue ID with its
guidance and remediation. The existing repository policy check validates
the registry.

For fast feedback while editing, use
[`cargo xtask verify --stage edit --base origin/main`](../docs/development/verification.md#edit-stage).

Before committing, run
[`cargo xtask verify --stage commit --base origin/main`](../docs/development/verification.md#commit-stage).

## Registry

The [executable catalogue](../harness/xtask/src/verification/catalogue.rs)
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

## Executable registration

Run `cargo xtask executables --base origin/main` for focused feedback. The
Architecture policy group runs the same guard with its required trusted base.
It inventories committed, staged, and reviewable working-tree files. Reports
include paths and target identities, with fingerprints for inline code bodies.

The [executable registry](executables.json) has two collections. The
`baseline` records legacy surfaces by exact path, kind and target. It is
frozen against the trusted base. Use `registrations` for additions, including
renames and previously plain files that gain a shebang or executable bit.
Stage a new registration with its corresponding surface.
For each entry record an owner, a concrete purpose, a class and a disposition.
Use the `fleetiq-platform` owner and one of `application`, `operation`, or
`harness`. Keep `unclassified` with `review` for legacy entries whose owners
must resolve mixed responsibilities. Other dispositions are `retain` and
`consolidate`. Classify new harness surfaces against the executable catalogue
by including its ID in `sensor` and pairing that check in the harness manifest.

| Kind | Path and target |
|---|---|
| `script` | Exact path with an empty target. Includes recognized script extensions, interpreter shebangs, Git or filesystem execute bits, symlinks and Git links. |
| `task` | Exact package script or repository Cargo alias name. A new Make, Just or Taskfile configuration is registered as `definition:<sha256>` of the file bytes until the platform adopts target-level discovery for that runner. |
| `ci` | Exact workflow or action path and `<job>:<kind>:<sha256>:<occurrence>`. Inline `run` and GitHub Script bodies use SHA-256 of the trimmed parsed YAML string. Occurrences count identical bodies within a job. Composite actions use `composite` as the job. |

Use the exact target from a guard failure when registering a new surface.
Changing the content of a registered script at the same path keeps its identity.
A changed inline command has a new identity and needs review. Workflow templates
are covered by the existing generated-workflow parity checks. Shell code embedded
in arbitrary application files and external workflows are outside this inventory.

Prefer an existing command before registering another. Keep protected publish
commands classified as operations. Their redesign remains in #464. The
`/harness/` CODEOWNERS rule routes registry changes for review. Required
independent approval and status checks depend on GitHub repository settings.

## Application isolation

The harness has its own [Cargo workspace](Cargo.toml) and lockfile. The
repository alias still exposes `cargo xtask verify`. The root Cargo workspace
contains the application, its protocol examples and its application tests.

From a clean committed checkout, with the normal migrated PostgreSQL and
Docker object-store test prerequisites, run `cargo xtask app-isolation`.
The Rust verification group runs this proof too. It archives the commit,
removes `harness/` in a temporary tree, and runs the locked workspace build,
the complete locked Nextest CI profile, and the locked doctests. Its target
directory belongs exclusively to the temporary tree. Existing application
fixtures, committed contracts, Wasm sources, and Nextest scheduling limits
are included. The regular Rust gate retains its application measurements.

The harness lockfile has independent supply-chain and Rust checks. Local
harness sensor tests are available through
`cargo test --manifest-path harness/Cargo.toml --locked`. The root lockfile
continues to determine [replay execution compatibility](../docs/operations/protocol-replay.md#upgrades-and-compatibility).
