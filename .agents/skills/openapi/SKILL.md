---
name: openapi
description: Maintain FleetIQ's modular backend-owned OpenAPI contract when changing documented HTTP routes, parameters, responses, examples, or client generation inputs.
---

# Maintain the HTTP contract

Read [the HTTP contract guide](../../../contracts/http/README.md) and the relevant server route and contract tests before editing. The modular YAML files are source; `contracts/http/dist/openapi.json` is the committed generated artifact consumed by Rust tests and client generators. Do not edit the bundle directly.

## Placement and references

```text
contracts/http/
  openapi/openapi.yaml                    # Root metadata, security, path and component indexes
  openapi/paths/<domain>/<resource>.yaml # One registered path item per file
  openapi/components/schemas/<Name>.yaml
  openapi/components/parameters/<name>.yaml
  openapi/components/responses/<Name>.yaml
  openapi/components/headers/<name>.yaml
  openapi/components/security-schemes/<name>.yaml
  dist/openapi.json                       # Generated standalone artifact
```

Use lowercase kebab-case for path and parameter files and the established component name for schema and response files. Every new path and component must have a matching `$ref` entry in `openapi/openapi.yaml`; place each path under its owning domain directory. References use forward slashes and a relative file path from the referring file, for example `./AssetTypeRef.yaml` inside a schema or `../../components/responses/NotFound.yaml` inside a nested path file. Avoid absolute paths, URLs, and references into the generated bundle.

Keep path items about HTTP behavior. Never add an inline schema longer than 15 YAML lines inside a path definition; extract it to `components/schemas/` and register it in the root. Reuse named parameters and responses when semantics really match. Preserve exact property types, `required` lists, operation IDs, route patterns, parameter locations and constraints, response status meanings, security declarations, and examples unless the API behavior intentionally changes.

## Update and verify

1. Edit the owning path and component files and root index together. Update affected server contract tests and response examples in the same change.
2. From the repository root run `pnpm install --frozen-lockfile`, `pnpm lint:openapi`, then `pnpm bundle:openapi`. Commit the regenerated `dist/openapi.json`.
3. Run `pnpm check:openapi` to verify lint and byte-for-byte bundle freshness. Run the applicable Rust contract tests and repository gate from [CONTRIBUTING.md](../../../CONTRIBUTING.md#verification).

The configured workload bearer is server-side; browser clients use the documented OIDC session mode under one HTTPS origin. Publishing a route alone does not qualify its deployment.
