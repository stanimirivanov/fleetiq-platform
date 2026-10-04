# HTTP contract artifacts

## TL;DR

The backend owns this OpenAPI 3.1 artifact. It currently describes the version-discovery and bounded tenant asset-catalogue routes needed by the first web/mobile slices. It is an explicitly partial v1 description, not a claim that other implemented routes do not exist.

[The UI baseline](fleetiq-v1-ui-baseline.openapi.json) is committed source, not output copied from a running instance. Its info.version tracks revisions of this documented subset; the path major version remains v1. The baseline records the current configured bearer credential only as a temporary protected-route mechanism. Browser or mobile production login must wait for the approved identity integration. Synthetic examples are safe to use as client fixtures.

When changing a described route, update the artifact, its response examples, server compatibility tests, and the client-consumption plan in the same change. Frontends should pin an exact backend commit or released artifact before generating transport types, validate fixture payloads, and never treat generated wire types as trusted domain values. Add more routes in small, separately reviewed slices.

OpenAPI 3.1 uses JSON Schema 2020-12 for Schema Objects. The server contract tests validate examples against their schemas and compare examples with in-process HTTP responses; this does not yet prove every undocumented route or every possible response variant.
