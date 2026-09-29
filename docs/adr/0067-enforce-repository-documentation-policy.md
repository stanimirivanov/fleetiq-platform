# ADR 0067: Enforce repository documentation policy

## Status

Accepted, 29 September 2026.

Supersedes: none. Superseded by: none.

## Context

FleetIQ records contributor workflow, documentation structure, architecture
decisions, milestones, and review expectations in the repository. These rules
guide implementation, but most of them depend on a contributor noticing drift
during review. A renamed heading can break inbound links, a new long guide can
omit its summary, an ADR can be left outside the index, and a GitHub template
can diverge from the canonical issue structure without failing the quality
gate.

ADR 0066 established an executable check for Cargo workspace boundaries. That
check deliberately covers dependency facts rather than documentation and
planning policy. FleetIQ also needs a fast sensor for repository guidance
before the planned architectural refactoring adds more capabilities and
decisions.

The repository already has a tooling-only `fleetiq-xtask` package. Rust tooling
dependencies in that package do not enter the server's linked dependency
graph, and the architecture policy prevents production packages from depending
on it.

## Decision

Extend `fleetiq-xtask` with `cargo xtask repository`. The command discovers the
repository's Markdown documents and reports all deterministic policy violations
in stable path and line order. Each diagnostic identifies the rule, violation,
likely correction, and canonical policy location.

The command checks:

- repository-local Markdown paths, exact path capitalization, and generated
  heading anchors without accessing the network;
- the documented `TL;DR` threshold, while exempting templates and historical
  ADRs whose fixed structure already separates the decision from its context;
- ADR filenames, heading numbers, required decision sections, contiguous
  numbering, a published high-water mark, grouped index coverage, and parseable
  lifecycle declarations when present;
- milestone index numbering, a published high-water mark, and agreement with
  milestone boundary headings; and
- the milestone field and section structure of the issue and pull-request
  templates against the canonical contributor policy.

Mechanically checked template prompts, required ADR headings, lifecycle
metadata and index entries, and milestone index or boundary content use plain
Markdown so the checker does not need to infer browser visibility from raw
HTML. Raw HTML comments remain available for author-only instructions and do
not satisfy required fields.

Use `pulldown-cmark` for Markdown structure and link events rather than
maintaining a partial Markdown grammar. Keep path resolution, repository
inventory, and FleetIQ-specific policy in `fleetiq-xtask`. Raw HTML links and
external URLs remain outside the local command because their validation either
requires another grammar or introduces network-dependent results.

Make the documentation index a task-oriented route to canonical sources and
record the harness's feedback tiers in one development guide. Run repository
policy beside the existing architecture policy before database-backed CI jobs.
Keep the existing CI status names so this change does not silently alter branch
protection.

The issue and pull-request templates remain authoring aids. The checker proves
their required shape, not the semantic quality of completed issue or pull
request text.

## Consequences

Broken local documentation links, missing summaries, unindexed decisions,
milestone drift, and template drift fail before the slower PostgreSQL-backed
quality gate. Contributors receive enough context to correct a violation
without reverse-engineering the checker.

The tooling package gains Markdown parsing dependencies, while deployable
packages and runtime behavior remain unchanged. Adding a new mechanically
enforceable documentation rule requires updating its canonical policy, checker,
and focused tests together.

Semantic accuracy, stale prose, raw HTML links, and external URL availability
still require review or a later advisory sensor. Supply-chain policy and a
closed aggregate verification command remain separate changes because they
need their own dependency, license, advisory, and database-boundary decisions.

## Alternatives considered

Keeping documentation policy as a review checklist was rejected because the
same deterministic drift would continue to consume reviewer attention and
could remain unnoticed until after merge.

Copying the Argus regular-expression parser was rejected because Rust has a
maintained CommonMark event parser that handles inline and reference links,
code blocks, and source spans more reliably.

Using an external link-checking service was rejected for the inner loop because
it would add network availability, credentials, and rate limits to checks whose
local portion can be deterministic.

Reformatting every historical ADR into the current template was rejected
because accepted records are historical evidence. The checker validates their
shared structure and preserves the absence of lifecycle sections in older
records rather than inventing metadata after the fact.
