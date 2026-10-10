# ADR 0097: Protect published history and sensitive artifacts

## Status

Accepted, 10 October 2026.

## Context

The repository requires immutable PostgreSQL migration history and prohibits
committed credentials, but those rules previously depended on contributor
review. SQLx checksum validation detects changes only after application, and
ordinary tests cannot discover a credential removed in a later commit. The
approved harness hardening sequence calls for early, deterministic history and
secret sensors with explicit policy ownership.

## Decision

Add `cargo xtask migrations --base REV` using Git's object and index interfaces.
Compare the trusted base's SQL blobs with staged entries and working files.
Reject published changes, deletions, renames, and nonregular entries. Validate
new flat, unique, numerically forward migration names, including untracked
files. Require an explicit existing ancestor commit and a nonempty valid
published inventory. Preserve SQLx and PostgreSQL tests for SQL correctness.

Add `cargo xtask secrets [--base REV]` using pinned Gitleaks 8.30.1 rather than
implement a credential detector. A Node installer verifies reviewed official
archive SHA-256 digests and the binary version. Scan an isolated reviewable-file
snapshot, a raw staged-blob snapshot, and the explicit incoming commit range in
an isolated bare clone with forced text diffs. Reject reviewable fingerprint-ignore
files. Disable inline suppressions, baselines, ambient configuration, and diff
conversion hooks; checkout filters and binary attributes cannot hide staged
blobs. Preserve built-in default rules; only exact owned synthetic
value AND exact file exceptions are accepted by the existing TOML parser.
Real fixtures show the pinned detector global AND allowlist can skip a whole
file. Never pass those exceptions to the detector; enforce exact literal file
and extracted-value equality in the wrapper after detection.

Capture JSON findings without detector redaction in a private process pipe to
compare exact values. Never persist or forward that report, raw process output,
or match fields. Use the workspace's maintained serde_json library to parse
findings and expose only sanitized relative file paths, rule IDs, and numeric
line numbers. Public output remains redacted. Its new direct tooling edge is
explicit in the architecture policy; no production dependency or crate changes.
Run real-detector bypass fixtures separately from unit tests so a missing tool
cannot turn the scanner's verification into a skip.

In CI, fetch full history and use the event's PR base SHA or main push predecessor
SHA. Do not infer HEAD^ or fall back on a missing, zero, or unrelated base. Scan
with both incoming and trusted-base detector policies when the latter exists.
The first installation is explicitly reviewed bootstrap policy; it checks the
current state and incoming range without claiming a full legacy history audit.

Route policy, migrations, sensor code, tool pins, and workflow changes through
CODEOWNERS and require explicit policy review in contributor guidance. Required
Code Owner approval and required status checks are GitHub settings that must be
verified by an administrator; checked-in files cannot enforce them alone.
Document this trust boundary rather than claiming the sensors protect their own
implementation from unreviewed alteration. Keep the agent entry point concise
and route details to [harness protection](../development/harness-protection.md).

## Consequences

Published migration immutability and incoming secret detection become early
CI gates without a database. Contributors receive safe correction locations
and can run the same pinned detector and fixtures locally. Reviewed exact
synthetic fixtures retain usefulness without exempting whole files or future
credentials. Current files, staged content, and incoming history are distinct
inputs; missing prerequisites fail explicitly.

Checks require trusted Git history, the pinned external binary, and network for
installation. Gitleaks coverage is finite; this does not establish comprehensive
security, a clean legacy history, or external merge permissions. Local base
freshness and administrative protection settings remain explicit responsibilities.
The later reproducible verification/recovery stage remains separate. No HTTP
payload, persisted meaning, migration bytes, or production behavior changes.

## Alternatives considered

Relying on SQLx checksums or review leaves migration drift visible only after
application or human inspection. Comparing published blobs before database
compilation closes the structural gap.

A custom regex credential detector reinvents maintained rules and would miss
formats and encodings. Pinned Gitleaks supplies detection; the narrow wrapper
owns input selection, configuration constraints, and safe evidence.

A global baseline or file exclusion would hide future leaks at the same location.
Exact synthetic exceptions and trusted-policy rescanning preserve detection.

A new orchestration framework or third-party merge bot adds another authority.
The existing xtask and CI job supply sensors; repository settings provide the
separate review trust boundary.
