# Published history and secret protection

## TL;DR

- Fetch trusted remote history, then run `cargo xtask migrations --base origin/main`
  and `cargo xtask secrets --base origin/main` before handoff.
- Install checksum-verified Gitleaks 8.30.1 with
  `node scripts/install-gitleaks.mjs`; run `cargo xtask secrets --fixtures`.
- Published migration bytes are immutable. Secret findings expose only the
  relative file, rule, and line; never copy credentials into review evidence.
- Scanner exceptions match one exact synthetic value in one exact file.
  Harness owners review every policy change. Required GitHub reviews and checks
  depend on external repository settings, which are not verified by these tools.

## Commands and prerequisites

Use the pinned Rust toolchain, Git, and Node.js 22.12 or newer. The installer
needs HTTPS access to the official release and a `tar` executable supporting its
archive format. It verifies a reviewed SHA-256 archive digest and exact binary
version before installing under ignored `target/tools/gitleaks`. Linux and
Windows x64 and arm64 archives are supported. Other platforms can install the
exact version through their supported tooling; the sensor rejects any other
version on PATH. No PostgreSQL is needed.

```powershell
git fetch origin
node scripts/install-gitleaks.mjs
cargo xtask migrations --base origin/main
cargo xtask secrets --base origin/main
cargo xtask secrets --fixtures
```

For immediate file and index feedback, `cargo xtask secrets` omits incoming
commit history. That focused command does not replace the base-qualified
handoff check. `cargo test -p fleetiq-xtask --locked` exercises the structural
and policy parser fixtures; the separate `--fixtures` command exercises the
actual pinned detector and fails if it is absent or ineffective.

## Trusted base selection

The base must identify the published state this change builds on, not a
revision selected to avoid a finding. Both sensors resolve the supplied
revision to a commit and require it to be an ancestor of HEAD. Migration
validation also requires a nonempty, valid published migration inventory.
Missing, all-zero, unavailable, or unrelated bases fail. There is no fallback
to HEAD, HEAD^, or an inferred merge base.

Local contributors fetch the current trusted `origin/main`. CI checks out
complete history with `fetch-depth: 0` and uses the event's pull-request base
SHA, or the push event's `before` SHA on main. The synthetic PR merge commit's
first parent is not the source of trust. Record the exact resolved base when
reporting verification. A new branch with no published base or an unavailable
force-push predecessor requires explicit maintainer investigation rather than
an automatic pass.

CI history protection still depends on a trustworthy runner, Git executable,
event metadata, and reviewed sensor implementation. A local actor can pass an
older ancestor manually; the local CLI cannot attest that a remote branch is
current. CI supplies the authoritative event base.

## Migration immutability

The sensor reads published SQL blobs from the base and compares both index
entries and working-tree bytes. Editing a staged migration and then restoring
only the working file still fails. Deletions, renames, symlinks, nonregular
entries, and changes to published content fail, including comment-only changes.
The LF policy in `.gitattributes` keeps SQL bytes consistent across platforms.

New migrations are flat files under `migrations/`, including untracked additions,
with `<version>_<lowercase_snake_case>.sql` names. A version has at least four
digits, is a positive signed 64-bit integer, is unique, and is greater than the
largest published version. Gaps are permitted; backdated additions and duplicate
versions are rejected. The base itself must satisfy inventory policy.

Correct mistakes with a forward migration. Restore accidental published-file
changes before adding it; never rewrite already-applied SQL or its checksum.
See [SQL migration criteria](sql-migrations.md) for schema documentation,
SQLx regeneration, database tests, and operational migration review. This sensor
proves file history and ordering, not SQL correctness or deployment safety.

## Secret scanning

The sensor obtains tracked and nonignored untracked candidates from Git,
including force-tracked files in otherwise ignored locations. It copies regular
working files to a private temporary snapshot, scans raw staged blobs in a second
snapshot, and, when given a base, scans `base..HEAD`. Secrets added and subsequently removed
within that incoming range still fail. Ignored untracked local files such as
`.env` and `target/` are outside this reviewable-file scan; do not mistake it
for a scan of the entire workstation. Candidate symlinks and substituted parent
directories fail rather than reading outside the checkout.

Gitleaks runs with explicit built-in default rules, inline allows disabled, a
controlled empty ignore file, and no baseline. Ambient Gitleaks configuration
is removed. Reviewable fingerprint-ignore files are forbidden, including nested
and case-varied names, because upstream implicitly discovers them despite the
empty-ignore flag. `gitleaks:allow` comments cannot suppress findings. History
scans use an isolated bare Git clone with forced text diffs and no worktree ignore
file. Raw staged blobs bypass diff attributes, text conversion, and checkout
filters; history diff/text conversion hooks are disabled for detector input.

The pinned detector's global AND allowlist can skip a whole file before comparing
its value; the real bypass fixture discovered this behavior. Repository exceptions
are therefore never passed to Gitleaks. The Rust wrapper validates restricted
literal policy and applies exact relative-file AND extracted-secret equality
in memory. No regex engine or credential detector is reimplemented.

To compare exact values, the child process sends JSON without detector redaction
through a private captured stdout pipe (`--redact=0 --report-path -`). Neither
this JSON nor raw stdout/stderr is printed, written to a report file, or included
in errors. The maintained JSON parser exposes only sanitized relative paths,
rule IDs, and numeric line numbers after exception filtering. Public diagnostics
are always redacted; temporary candidate/index snapshots are removed at check
completion. Do not add debug logging of report values or inherited child output.

Both the current policy and a policy present in the trusted base must pass.
An incoming policy relaxation cannot hide a finding detected by the published
policy. The first installation is a bootstrap because earlier commits have no
scanner policy: it checks the current tree, index, and incoming range under the
reviewed new policy. This does not certify all historical commits predating that
base; a separate full-history audit must establish such a claim. The command
reports whether incoming history was checked, including an empty range.

Default detector rules stay enabled. The configuration parser rejects custom
rule replacement, disabled defaults, broad regular expressions, path-only
allowlists, commit exclusions, stopwords, and unowned exceptions. Each exception
has one anchored literal path AND one anchored literal secret, a meaningful
`Owner:`, `Reason:`, and `Review trigger:` description, and `regexTarget = 'secret'`.
Regex metacharacters must be escaped; the exact `[/\\]` path separator is
permitted for Windows and Unix paths without allowing wildcards. The existing
deterministic OIDC signing fixture is nonproduction test data; its exact exception does not exempt its
file or future keys. Review an exception separately from ordinary feature work.
When replacing a fixture, first land an additive exact exception that retains
the existing one, then change the fixture in a subsequent PR. Both policies
continue to apply, so changing a key and its new exception together cannot
bypass published policy. Remove obsolete exceptions after the transition.

The eight detector and input-policy scenarios verify inline suppression,
forbidden fingerprint-ignore files, an exact synthetic exception, another value
in that file, the excepted value in another file, a staged secret hidden by a
clean worktree and binary attributes, a binary-marked secret added then removed
in history, and an incoming policy relaxation checked against trusted policy.
Unit fixtures cover malformed policy, missing history, safe diagnostics,
raw blob evidence, and linked-directory boundaries.

On a finding, remove the credential and follow [SECURITY.md](../../SECURITY.md)
for private reporting and rotation or revocation. Removing it from the current
file alone does not erase an incoming commit. Agree any necessary unpublished
history correction with maintainers; do not rewrite published history casually.
Never add a baseline, inline allow, or directory exclusion just to obtain a pass.
Selected-file counts report inputs, not bytes examined by every rule. Built-in
detector exclusions for some paths and formats remain in effect; see the
[pinned default configuration](https://github.com/gitleaks/gitleaks/blob/v8.30.1/config/gitleaks.toml).
Detector limitations remain: a pass does not prove that every credential format,
customer-data leak, runtime configuration, or preexisting commit is safe.

## Ownership, merge controls, and maintenance

[CODEOWNERS](../../.github/CODEOWNERS) routes policy, migration, CI, dependency,
and sensor changes to the repository maintainer. It also owns itself. Explain
any weakened rule, exception, or tool pin explicitly in the PR and prefer a
separate harness change for gate relaxations. The first policy introduction
must receive bootstrap review.

The administrator must confirm that the listed owner has sufficient repository
access and appoint an independent authorized owner or team before requiring
Code Owner review. GitHub forbids a PR author from approving their own PR;
the current single owner provides routing and cannot satisfy their own required
review. Verify that the main-branch ruleset requires Code Owner review, rejects
stale approvals after new commits, and requires the existing `Architecture
policy`, supply-chain, Rust, HTTP contract, and runtime checks as applicable.
Do not bypass those controls for incoming policy changes. These external settings
are unverified here: a checked-in CODEOWNERS file routes review but cannot
activate required approvals or defend a modified workflow by itself. See the
[GitHub Code Owners reference](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-code-owners)
and [required-review rules](https://docs.github.com/en/pull-requests/how-tos/review-pull-requests/approving-a-pull-request-with-required-reviews).

Gitleaks is a feature-complete upstream detector receiving maintenance and
security fixes. Review upstream release provenance and archive checksums when
changing its pin; update the installer and Rust version assertion together,
then run the real bypass suite and current/base scans. Do not fetch a moving
checksum as the trust anchor. See the [pinned official release](https://github.com/gitleaks/gitleaks/releases/tag/v8.30.1)
and [dependency management](dependency-management.md).

A missing tool, checksum mismatch, invalid base, malformed policy, unreadable
input, detector failure, or parsing failure is a failed check, never a pass.
Report unavailable verification with its blocker and residual risk using
[CONTRIBUTING.md](../../CONTRIBUTING.md#verification). No runtime product
behavior, HTTP schema, SQL migration, or production dependency is changed by
these development controls.
