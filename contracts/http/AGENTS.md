# Backend-owned HTTP contract work

Technical constraints for this scope; the repository root `AGENTS.md` owns workflow routing.

## TL;DR

The modular YAML is authoritative. Keep it aligned with server behavior and
the committed generated bundle; begin with these task-specific routes.

## Read before editing

- [HTTP contract guide](README.md)
- [OpenAPI maintenance skill](../../.agents/skills/openapi/SKILL.md)
- [Versioned HTTP boundary](../../docs/adr/0048-establish-versioned-http-api-boundary.md)
- [Verification groups and evidence](../../docs/development/verification.md)
- [Verification and acceptance](../../CONTRIBUTING.md)

## Module constraints

Follow the skill's placement, relative-reference, registration, and schema
extraction rules. Update owning paths, components, examples, and relevant
server contract tests together. Preserve payload requirements, operation IDs,
parameter semantics, response statuses, and security declarations unless the
reviewed change intentionally evolves them.

Run `pnpm check:openapi` from the repository root. This includes sensor fixtures,
contract lint, structural policy, and bundle freshness. Regenerate intentionally
with `pnpm bundle:openapi` after a source change; never hand-edit the bundle.
Run applicable Rust compatibility checks and report unavailable database
verification honestly. Contract publication alone does not qualify deployment.
