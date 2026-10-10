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
`pnpm check:openapi` to run sensor regression fixtures, lint, and verify the bundle is current. Rust contract tests read
the bundle at compile time; generate it before running those tests after a
source edit.

OpenAPI 3.1 uses JSON Schema 2020-12 for Schema Objects. Contract tests
validate examples against their schemas and compare covered examples with
in-process HTTP responses. They do not prove every undocumented route, every
database state, or every possible response variant.
## Maintaining modular ownership

Register each source file exactly once in the root path or component index.
Extension fields and incidental references do not count as registration.
Component keys match their filename and category. Path files use
`paths/<lowercase-kebab-domain>/<lowercase-kebab-resource>.yaml`; parameters
and headers use lowercase kebab-case names. The source tree contains only
regular `.yaml` files. Source entries, including ancestors inside the checkout,
cannot redirect through symlinks or Windows junctions.

Every reference names a contained source file using `./` or leading `../`
segments and forward slashes. Avoid absolute paths, remote URLs, URL-encoded
filenames, query options, redundant traversal, and filename casing changes.
Root index references name whole files; nested references may use JSON Pointer
fragments, whose resolution is checked by Redocly. Each registered resource
must resolve to its own source document. Whole-file aliases can hide an
intermediate reference edge when a resolver collapses their chain; express
schema reuse through explicit composition, such as `allOf`, instead.

Path and operation parameters reference registered parameter components.
Inline path schemas may contain at most fifteen expanded YAML mapping or
sequence entries. Nested entries count toward the budget, even in a one-line
flow mapping or when a YAML alias repeats a structure. Comments, scalar text,
and quoting do not add structural entries. Larger schemas belong in registered
schema components. Recursive YAML aliases fail; ordinary schema `$ref` reuse
remains supported.

## Sensor regression tests

`pnpm test:openapi` runs database-independent Node tests without rewriting the
contract. The fixtures cover accepted YAML syntax, real root registration,
reference containment and casing, parameter extraction, schema complexity,
malformed YAML, symlinks/junctions, reference-chain ownership, and byte-exact
bundle freshness. Live Redocly fixtures use the repository configuration and
require the `fleetiq/layout` diagnostic for layout violations, so removing or
disabling the custom rule fails the test suite.

The layout rule uses Redocly's public parsed-document visitor API rather than
an independent YAML parser or line regular expressions. The ordinary lint and
freshness wrappers use the pinned installed CLI, reject ambient Redocly command
option overrides, and clean temporary generated output on success or failure.
The existing two reviewed OAuth redirect suppressions remain unchanged. Only those exact rule/path/pointer exceptions are accepted; ignore files cannot suppress structural findings.
Failed launches, nonzero exits, missing output, and stale bundles fail the
check and never replace the committed artifact. `pnpm check:openapi` includes
these tests in the same OpenAPI gate used locally and in CI.
The structural walker visits the supported registered resource kinds and real
JSON Schema keyword containers. Media examples, named `Example.value`, schema
`const`/`default`/`enum`/`examples`, and specification extension payloads are
opaque JSON data, so reserved-looking property names do not become API rules.
The fifteen-entry budget still counts every expanded entry of an inline schema.
Named Example Object references and actual schema references remain resolved
and checked. Path parameter references must target registered files under
`components/parameters/`, including when a contained fragment otherwise has
a valid Parameter Object shape.

Redocly's documented `resolve.doNotResolveExamples` setting prevents singular
example payloads from being interpreted as references. Its public type-extension
metadata marks only literal annotation and extension values as nonresolvable;
existing normative field validators and `no-unresolved-refs` remain enabled.
Paired live fixtures prove literal data stays intact and missing real references
still fail. This is a traversal configuration, not an independent schema validator.