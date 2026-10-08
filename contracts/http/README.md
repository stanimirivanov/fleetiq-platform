# HTTP contract artifacts

## TL;DR

The backend owns this partial OpenAPI 3.1 description of `/api/v1`. It
publishes version discovery, bounded asset catalogue and exact asset reads,
pinned semantic definitions, and bounded bitemporal relationship and
signal-binding snapshots. Other implemented routes remain outside this
client-facing subset. Protected reads still use a configured workload bearer
and current tenant `metadata.read` access; that credential is not browser
authentication.

[The UI baseline](fleetiq-v1-ui-baseline.openapi.json) is committed source,
not output copied from a running instance. Its `info.version` tracks revisions
of this documented subset; the path major version remains v1. The current
configured bearer is a temporary server-side adapter for one principal.
Browser and mobile production login require a separate approved identity
integration. Never embed the workload credential in a web or mobile client.

Relationship snapshots are scoped to one asset. Target-binding snapshots are
scoped to one asset, and source-binding snapshots to one decoded signal.
Each uses explicit effective and known times, bounded pages, and an opaque
continuation cursor. These diagnostic reads are not a whole-graph query,
current semantic property-state projection, or live telemetry feed. Pinned
definitions identify the exact version named by an asset, relationship, or
binding. Synthetic examples are suitable for client fixtures.

When changing a described route, update the artifact, response examples,
server compatibility tests, and client-consumption plan in the same change.
A frontend should pin an exact merged backend commit or released artifact,
regenerate transport types, validate unknown payloads at its feature boundary,
and keep sample mode visibly separate from live data. Pinning this expanded
contract allows typed mock-driven UI work; production browser reads remain
unconfigured until browser-safe identity and authorization are delivered.

OpenAPI 3.1 uses JSON Schema 2020-12 for Schema Objects. Contract tests
validate examples against their schemas and compare covered examples with
in-process HTTP responses. They do not prove every undocumented route, every
database state, or every possible response variant.