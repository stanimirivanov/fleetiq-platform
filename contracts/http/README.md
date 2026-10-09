# HTTP contract artifacts

## TL;DR

The backend owns this partial OpenAPI 3.1 description of `/api/v1`. It
publishes version discovery, bounded asset catalogue and exact asset reads,
pinned semantic definitions, and bounded bitemporal relationship and
signal-binding snapshots. Other implemented routes remain outside this
client-facing subset. The additive baseline includes operator OIDC login,
callback, session discovery, and sign-out. Protected reads use either browser
cookies or configured workload bearer plus current tenant `metadata.read` access;
a server listener selects exactly one mode.

[The modular UI baseline](openapi/openapi.yaml) is the committed source of
truth, not output copied from a running instance. Paths and reusable components
live in adjacent domain and component files. The committed
[bundled JSON artifact](dist/openapi.json) is generated from that entrypoint
for Rust contract tests and downstream client/server tooling; do not edit it
by hand. A contract change updates the source, regenerates the bundle, and
checks both together. `info.version` tracks revisions of this documented
subset; the path major version remains v1.

The current configured bearer is a temporary server-side adapter for one
principal. The [browser identity boundary](../../docs/operations/browser-identity.md) uses
provider-neutral OIDC and same-origin server sessions. Native mobile identity
remains separate. Never embed the workload credential in a web or mobile
client.

Relationship snapshots are scoped to one asset. Target-binding snapshots are
scoped to one asset, and source-binding snapshots to one decoded signal.
Each uses explicit effective and known times, bounded pages, and an opaque
continuation cursor. These diagnostic reads are not a whole-graph query,
current semantic property-state projection, or live telemetry feed. Pinned
definitions identify the exact version named by an asset, relationship, or
binding. Synthetic examples are suitable for client fixtures.

When changing a described route, update the modular source, bundled
artifact, response examples,
server compatibility tests, and client-consumption plan in the same change.
A frontend should pin an exact merged backend commit or released artifact,
regenerate transport types, validate unknown payloads at its feature boundary,
and keep sample mode visibly separate from live data. Pinning this expanded
contract allows typed mock-driven UI work; connected UI activation requires a qualified OIDC deployment, provisioned
operators, and the frontend connection slice.

From the repository root, run `pnpm lint:openapi` to lint the modular source,
`pnpm bundle:openapi` to regenerate the committed standalone JSON, and
`pnpm check:openapi` to verify the bundle is current. Rust contract tests read
the bundle at compile time; generate it before running those tests after a
source edit.

OpenAPI 3.1 uses JSON Schema 2020-12 for Schema Objects. Contract tests
validate examples against their schemas and compare covered examples with
in-process HTTP responses. They do not prove every undocumented route, every
database state, or every possible response variant.