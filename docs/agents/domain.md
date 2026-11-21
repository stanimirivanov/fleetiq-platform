# Domain docs

This repository uses a single domain context for backend, persisted model and published HTTP contracts.
Read root `GLOSSARY.md` when present and relevant accepted decisions in
`docs/adr/` before exploring the affected domain. These decision records remain
in their existing location and retain their identities and history.

Use defined terminology in issues, code discussions and documentation. Surface
conflicts with accepted decisions; supersede decisions explicitly rather than
silently rewriting their meaning.

The `domain-modeling` skill creates glossary entries and ADRs when terms or
decisions are resolved. If the glossary or decision directory is absent, proceed
without scaffolding empty documents. A future `GLOSSARY-MAP.md`, if explicitly
introduced, routes to the relevant bounded contexts.

Shared FleetIQ product semantics belong to `stanimirivanov/fleetiq-platform`;
cross-application installation and integration decisions belong to
`stanimirivanov/fleetiq-deploy`. Reference the owner's documents rather than copy
definitions. Keep repository-specific extensions here. Infrastructure terminology
remains product-neutral. Use repository URLs for external references so this
checkout works independently of sibling directories.

Record a durable decision when changing compatibility, persisted meaning, security,
deployment topology, foundational technology or ownership. Cross-crate architectural choices also need a decision record.

For terminology, read the [canonical glossary](../../GLOSSARY.md). For module
ownership, contract boundaries and open decisions, read the [domain map](../architecture/domain-model.md).
