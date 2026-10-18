# Reproducible verification and evidence

## TL;DR

- `cargo xtask verify --list` prints the authoritative check inventory; it runs
  no acceptance checks.
- `cargo xtask verify --group acceptance --base origin/main` coordinates the
  thirteen-command Rust acceptance gate.
- Focused groups support fast feedback. Passing a focused group is not a
  complete acceptance result, and contract or Compose checks may also apply.
- Missing prerequisites produce **not run** and a nonzero group exit. A started
  command returning an unsuccessful status produces **failed**.
- Reports under ignored `target/verification/` contain metadata, never process
  output, credentials, connection strings, or environment dumps. Ordinary tool
  output remains on the console and needs the same care as a direct invocation.
- Configure tools and PostgreSQL yourself. The runner installs nothing, applies
  no migrations, and infers no database health from `DATABASE_URL`.

## Authority and scope

[CONTRIBUTING.md](../../CONTRIBUTING.md#verification) owns which checks are
required. The executable [catalogue](../../crates/xtask/src/verification/catalogue.rs)
owns command arguments, groups, outcomes, tool versions, prerequisites, and
acceptance membership. Contributor and PR-template acceptance command lists
are checked against that catalogue. The runner compares the complete checked-in
CI workflow with its rendered reviewed template, including inventory-owned group
commands and tool versions. Action major tags are workflow-owned dependency
metadata, validated and substituted into fixed template slots. Accidental
omissions, disabled checks, or conditional
changes cannot silently create a different local or CI gate.

The runner coordinates the inventoried tools, including Nextest for runtime
test scheduling; it adds no independent test implementation or business policy. Each check remains individually runnable for investigation.
Execution uses fixed program and argument vectors. Repository-owned xtask checks
launch the current compiled xtask with the equivalent task arguments, avoiding
recursive Cargo rebuilds of the executable that Windows has locked. Other checks
invoke their inventoried tools directly. Child tools do not inherit Cargo run's
launcher package metadata; that metadata otherwise changes cargo-machete's
argument interpretation. Toolchain, jobserver, and database configuration remain
available to the checks. Reports name this execution adapter and
record its actual argument vector alongside the canonical contributor command.
Trusted base references are
resolved to ancestor commit IDs before being passed to the history checks.
Rejected CLI input is not echoed, and arbitrary command or output-path options
are not supported.

This catalogue and its sensors remain reviewable repository policy. Incoming
code can change them; they cannot certify their own integrity. CODEOWNERS routes
review, while required independent approval and required status checks depend
on externally configured GitHub protections. Those settings remain unverified;
see [harness protection](harness-protection.md#ownership-merge-controls-and-maintenance).

## Select an explicit group

List all check metadata with `cargo xtask verify --list`. The output is JSON for
inspection; it is not a result report. Run a group from the repository root:

| Group | Command | Selected checks |
|:--|:--|:--|
| Architecture | `cargo xtask verify --group architecture --base origin/main` | Repository policy, architecture policy, migration immutability, current/index/incoming secret scan, real scanner fixtures, and xtask sensor tests |
| Supply chain | `cargo xtask verify --group supply-chain` | cargo-deny and cargo-machete |
| Rust | `cargo xtask verify --group rust` | Formatting, SQLx metadata, workspace Clippy, bounded workspace tests, separate doctests, and rustdoc |
| OpenAPI | `cargo xtask verify --group openapi` | Modular contract linting, reference and extraction rules, and committed bundle freshness |
| Acceptance | `cargo xtask verify --group acceptance --base origin/main` | Exactly the thirteen canonical acceptance checks in contributor-policy order |

Scope is explicit; the runner does not guess from changed files. Acceptance
includes scanner fixtures, but focused xtask sensor tests and OpenAPI remain
separate inventory entries. The CI architecture group runs the sensor tests,
and the full workspace Nextest command also includes them. The Windows harness
job also runs the inventoried repository, architecture, and sensor-test commands,
followed by the same OpenAPI group. Its extra coverage does not replace the
Linux, PostgreSQL-backed, or Compose gates.

A selected group's success says only that all selected checks passed. When the
published HTTP contract changes, the OpenAPI group is also required. Deployment,
configuration, startup, readiness, or container changes still require the
existing Compose smoke checks. The runner does not absorb those operational
checks or claim that a thirteen-command acceptance run proves production
readiness. Use the [harness tiers](harness.md#cost-and-timing-tiers) and the
canonical workflow to determine the complete applicable gate.

## Prepare prerequisites

The inventory reports the required versions. Rust is pinned to 1.98.1, Gitleaks
to 8.30.1, cargo-deny to 0.20.2, cargo-machete to 0.9.2, SQLx CLI to 0.9.0, cargo-nextest to 0.9.145, and
pnpm to 12.5.1. Node.js uses the reviewed major version 24; Git must be installed.
The runner probes versions and records only numeric version tokens. An
unavailable or unsupported tool is a blocker, not a passing check. Tool
installation remains explicit in the [development guide](README.md),
[dependency management guide](dependency-management.md), and
[scanner installation guide](harness-protection.md#commands-and-prerequisites).

For architecture or acceptance, supply a current trusted published base with
`--base`. Fetch the latest history before using `origin/main`. CI provides the
PR event base SHA; do not select an older ancestor to hide
published changes. The runner records the resolved ID rather than the caller's
free-form reference. Missing, unrelated, or unavailable history blocks the
checks that need it. Base freshness remains the caller's responsibility.

For database-backed checks, `DATABASE_URL` must be nonempty and point to fully
migrated PostgreSQL 18.6. **`SQLX_OFFLINE` must be unset**, including values such
as `false`: online acceptance must not substitute cached query descriptions for
schema checking. Use the setup and migration instructions in the
[development guide](README.md#local-setup). The runner checks only these
configuration conditions. It does not contact PostgreSQL during preflight,
validate its version, verify migrations, or assert readiness. The actual SQLx,
Clippy, tests, and rustdoc commands establish their own results.

For OpenAPI, install the locked dependencies explicitly with
`pnpm install --frozen-lockfile`. The runner requires the installed tools and
pinned Node/pnpm versions; the contract checker remains the authority for the
contract and bundle.

cargo-deny needs current advisory data and may use the network. Other commands
may also need already installed dependencies or a usable service. The runner
cannot reliably distinguish a policy finding from every operational error
inside a started tool. A nonzero started command is **failed**, including an
advisory-fetch failure; explain the operational blocker when reporting it and
rerun when available. This status never implies that a security review ran to
completion or found a vulnerability.

## Execution and result semantics

Checks run in inventory order. A blocked or failed check does not prevent
independent later checks from producing evidence. The runner adds no automatic
retries, setup commands, package downloads, database migrations, or skip-to-pass
behavior. Underlying tools retain their ordinary behavior, including Cargo
resolution and advisory downloads. Executed Rust commands receive the same
warnings-as-errors environment as CI.

| Result | Meaning | Evidence |
|:--|:--|:--|
| Passed | The selected command started and returned success | Exit code zero and elapsed time |
| Failed | The command started and returned failure or was terminated | Numeric exit code when available; null for termination without a code |
| Not run | A required condition blocked execution, or the process could not start | Safe blocker description; no invented exit code or executed arguments |

The group exits unsuccessfully if any selected check failed or did not run.
Failure to create fresh evidence also fails the invocation. `complete_pass`
means all checks in that explicit group passed; it is not an acceptance claim
for a focused group. A failed check keeps its first observed result until a
separate invocation is run. Do not rewrite a previous report to hide failure.

## Evidence and privacy

Each invocation creates a distinct JSON file under ignored
`target/verification/`. Names identify the group, process, and timestamp; reports
are created afresh rather than overwriting stale results. The owned evidence
parents must be directories and may not redirect through a symlink or Windows
junction. There is no caller-selected artifact destination.

A report contains its schema version, selected group, complete-pass flag,
starting and ending Git HEAD and dirty-state observations, a run-level blocker
when source or CI consistency fails, resolved trusted
base, safe tool-version observations, fixed check metadata, execution arguments
with the resolved base, result, exit code where meaningful, and elapsed time.
It contains no captured stdout/stderr, raw version-probe output, command errors,
file contents, database credentials, connection URLs, or environment dumps.
The existing secret sensor separately retains its redacted diagnostics.

Ordinary check output is inherited by the console. It is not appended to JSON,
but terminal recording and CI logs may persist it outside this artifact policy.
Do not paste logs containing credentials, customer data, or production telemetry
into issues. Investigate a failed command using the safe location or structured
error supplied by the underlying tool, and sanitize any necessary shared detail.

HEAD and dirty-state observations are context, not content hashes or an
attestation. Two dirty trees can have the same observations, and files can
change during execution. A report does not prove that the current tree equals
the verified tree, that all tools examined every byte, or that policy is
independently trusted. Avoid concurrent edits during final verification,
review the resulting diff, and rerun affected checks after a change. A detected
HEAD or clean/dirty transition, unavailable final Git state, or changed CI wiring
fails the group; changes within an already-dirty tree remain a documented limit. CI artifact
handling and retention remain governed by the workflow and GitHub settings.

## CI and contributor handoff

The CI workflow runs only on pull requests to conserve this private repository's
GitHub Actions credits. Merging to main does not start a duplicate run. Direct
pushes to main are not validated by this workflow; passing PR checks remain the
merge-validation boundary. Other workflows and repository branch rules are
configured separately.

CI invokes the same architecture, supply-chain, Rust, and OpenAPI groups after
its explicit prerequisite setup. The additional `Windows harness sensors` job
executes the existing inventory-owned repository, architecture, and sensor-test
commands on Windows, and the same OpenAPI group with its Node fixtures. Windows
junction cases and Unix symlink cases run on their actual platforms without
silently skipping a failed fixture setup. The Windows JSON artifact covers only
its OpenAPI group; Rust fixture results remain in the step output and job status.
This extra job requires no database and changes no acceptance membership. Existing job names and the full applicable
gates remain in place; Compose smoke verification retains its own job. That job now qualifies one production OCI artifact, scans it and exercises actual development Compose using the same bytes; it runs the producer negative fixtures and retains redacted image evidence. The Windows job also executes these database-independent producer fixtures.
The six jobs start independently so a scanner installer or action failure in
architecture cannot skip Rust, supply-chain, OpenAPI, or Compose evidence.
Branch protection must require each applicable job; job independence does not
make an unsuccessful architecture result acceptable.

The reviewed [workflow template](../../.github/ci.yml.template) owns setup,
services, permissions, action repositories and positions, artifact handling, and
cleanup. Workflow-owned action major tags fill version-free template slots. Catalogue placeholders
supply group commands and pinned tool versions. `cargo xtask verify --check-ci`
compares the entire rendered document with the checked-in workflow, normalizing
LF/CRLF line endings. This checks repository wiring, not external GitHub behavior
or merge protections. The new `Windows harness sensors` status also needs explicit
administrator review when configuring required checks; adding a job does not
make it required for merge. Every group performs the same preflight comparison.

For an intentional CI policy change, edit the template or catalogue, review the
policy diff, then explicitly run `cargo xtask verify --write-ci`. Commit the
regenerated workflow and run `--check-ci`. Ordinary verification never regenerates
or silently repairs stale policy. A mutable template can itself weaken a gate,
so matching generated content never replaces independent policy review.

## CI resilience rules

### Separate policy from dependency metadata

The workflow is the only source of action versions. Each approved action uses
an `@v<major>` tag, such as `@v7`. The template contains named action slots,
not a second version list. The sensor validates the approved repository, tag
shape, and agreement across all uses before rendering those slots. All other
workflow content remains compared exactly, including permissions, commands,
conditions, setup inputs, services, and cleanup. No YAML parser or Node process
is required for this Rust sensor.

A major tag follows compatible action releases without routine workflow edits.
Review major-version updates and runtime requirements when changing a tag. The
CI jobs qualify the action code resolved for each run.

An action-only major-version update changes the workflow without editing the
template or running regeneration. The sensor does not fetch action source or
prove upstream provenance or compatibility. Review the official action release
notes and run the applicable checks before merge.

Structural or tool-version changes still update the template or catalogue,
then explicitly run `cargo xtask verify --write-ci` and `--check-ci`.
Regeneration preserves the workflow's action tags. Missing, malformed, or
inconsistent tags fail rather than guessing a ref, downloading an alternative,
or rolling a Dependabot proposal back. Ordinary checks never repair policy.

### Keep tool setup local to its owner

| Job | Explicit tools and state |
|:--|:--|
| Architecture policy | Rust and Git run CI, repository, and architecture policy checks before Node 24 installs checksum-verified Gitleaks |
| Windows harness sensors / OpenAPI contract | Rust, Git, pinned pnpm, Node 24, and locked local Redocly installation |
| Supply-chain policy | Rust and the inventoried cargo-deny/cargo-machete versions; no Node or npm setup |
| Rust quality gate | Repository-selected Rust, exact Nextest, SQLx CLI, Rust dependency caching, and migrated PostgreSQL; DATABASE_URL is scoped to this job |
| Local Compose smoke test | Docker Compose and the separately declared image build tools |

Rust checks must not read Node action outputs, resolve tools through npm/npx,
or require a package-manager cache hit. JavaScript action implementations use
their own declared runtime; that does not install Node tooling for Rust commands.
Each job owns its setup and runs without a `needs` edge to architecture.
The Rust job applies migrations with its pinned SQLx CLI, the same tool used in
local setup, rather than an ambient PostgreSQL client from the runner image.
Jobs select the Ubuntu 26.04 and Windows 2025 image families explicitly;
GitHub still updates each hosted image within its family.
Every applicable job remains a required review gate; dependency qualification
never implies permission to bypass architecture policy.

OpenAPI jobs use `pnpm install --frozen-lockfile` and the local locked CLI.
Their explicit pnpm cache is keyed by `pnpm-lock.yaml`; implicit npm cache
detection is disabled. Cache hits must never skip installation or verification.
Only the package store is cached, not node_modules or acceptance reports.
The Rust job uses Swatinem/rust-cache for registries and dependency build artifacts.
Its Rust environment hash includes toolchain, Cargo manifests/lockfile and configuration;
explicit inputs additionally isolate OS, architecture, execution policy, flags and target.
Only explicit debug build, deps and fingerprint directories are cached; whole-target
caching is disabled, excluding tools, Nextest output and verification reports.
Workspace crates and installed binaries are excluded. PRs restore existing cache
entries and never save them. The save condition remains restricted to successful
main pushes, which the PR-only workflow no longer schedules. Consequently, this
workflow does not refresh trusted-main Rust caches; entries may expire and later
runs may be cold. Alternate cache warming or PR-scoped writes require a separate
review. Never cache acceptance reports or treat a cache hit as verification.
The exact template rejects conditional checks, unreviewed cache inputs and PR cache writes. Docker BuildKit's existing locked
build cache remains separate from GitHub Actions caches.

### Diagnose, repair, and qualify

- Run `cargo xtask verify --check-ci` first when investigating workflow drift.
  A dependency-only update must pass; command or permission drift must fail.
- Exercise actual filesystem fixtures for pin-only updates and updates combined
  with disabled checks, altered setup, unapproved owners, or malformed refs.
- Publish mandatory JSON evidence when the verification step ran, including on
  failure. If prerequisite setup failed and verification was skipped, do not add
  a misleading missing-artifact failure; the failed setup still fails the job.
- Keep command stderr visible and the runner's passed/failed/not-run distinction.
  Do not dump environment values or credentials for diagnostics.
- Treat advisory failures independently of action metadata. Patch an affected
  dependency and qualify its owning adapter; do not add advisory ignores or
  suppress duplicate-version warnings to hide the failure.
- Before merge, require the applicable CI jobs on the actual candidate revision.
  Branch rules and independent owner approval are external controls: this
  repository cannot certify their settings or prevent an administrator bypass.
  No local sensor can execute a GitHub-hosted action. The Node 24 patch version
  and hosted runner images still change within their declared families, so
  local checks cannot guarantee identical environments or that an upstream
  action update is behaviorally safe.

At handoff, report the repository, milestone, issue content, assumptions, and
**passed / failed / not run** evidence using the
[completion policy](../../CONTRIBUTING.md#completion-report). Reference the
selected group and report when useful. Name any missing prerequisite and the
remaining risk; a passing architecture report cannot replace unavailable
PostgreSQL-backed verification. A fresh local report does not establish that CI
or protected-branch settings passed.

## Parallel test execution and coverage

The canonical runtime check is `cargo nextest run --workspace --all-features --locked --profile ci`.
Cargo doctests remain a separate required check: `cargo test --workspace --all-features --locked --doc`.
Rustdoc generation still runs afterward. The full gate now has thirteen checks;
no existing acceptance behavior is dropped. Supplemental Windows and architecture
sensor commands continue to use Cargo and do not require Nextest.

Install the exact local tool with `cargo install cargo-nextest --version 0.9.145 --locked`,
or use the [official prebuilt release](https://github.com/nextest-rs/nextest/releases/tag/cargo-nextest-0.9.145)
and verify its published SHA-256 before execution. CI uses taiki-e/install-action
with an exact version, checksums enabled and fallback disabled. Both new actions
follow the workflow-owned major-tag policy; neither introduces npm setup.

The reviewed `.config/nextest.toml` sets eight global slots, four shared slots for
all postgres/server tests and two Wasmtime slots. Every SQLx test retains its
isolated migrated database; explicit asynchronous races inside tests remain.
The CI profile inherits the default resource groups. Both profiles disable
retries and fail-fast; all selected tests run even after failures.

A test prerequisite parses and checks the entire supported execution shape,
including package classifications. Extra filters, retry rules, scripts, profiles,
missing groups, malformed input, and symlink/junction redirection fail closed.
The inventoried test process removes ambient NEXTEST_* options, including retry
and thread overrides, without printing values or removing database configuration.
A new database-bearing package must be added to the reviewed group and sensor
in the same PR. Policy owners review changes; the sensor does not attest itself.

Before changing runners again, reconcile each Cargo test binary's `--list` result
with Nextest's JSON inventory, including ignored flags, and verify doctests separately.
Record build versus execution timing and complete failures; never enable automatic
retries, skip slow tests or increase connection limits to manufacture a speedup.
Hosted cold and warm cache runs remain the authority for CI performance. See
[ADR 0104](../adr/0104-bound-parallel-rust-tests-and-cache-dependencies.md).
