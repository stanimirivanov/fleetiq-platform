# HTTP contract artifacts

## TL;DR

The backend owns this partial OpenAPI 3.1 description of `/api/v1`. It
publishes actor-scoped current tenant/action discovery, version discovery, bounded asset catalogue and exact asset reads,
immutable semantic definition publication/discovery, asset/device registration and ordered device reads, attributed relationship/binding
revision commands and exact revision reads, and bounded bitemporal relationship and
signal-binding snapshots. Other implemented routes remain outside this
client-facing subset. The additive baseline includes operator OIDC login,
callback, session discovery, and sign-out. Metadata reads use either browser
cookies or configured workload bearer plus current tenant `metadata.read` access;
publication, registration and revision commands require exact tenant `metadata.manage`. Protocol artifact storage/admission requires separate exact `protocol.manage`; baseline.13 expands the closed action vocabulary, so regenerate clients before delegating it. No existing grant is upgraded automatically. Registration atomically retains original actor/time evidence on exact retries; see [commissioning](../../docs/operations/metadata-registration.md). A server listener selects
exactly one mode. Operator discovery uses that same
authentication boundary and returns only the caller's current active tenant
grants, without requiring a tenant to be selected first.

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

Bundle freshness compares exact bytes. The generated `dist/openapi.json` uses LF,
and [the repository attributes](../../.gitattributes) preserve those bytes on
Windows and Unix checkouts, including Git's `core.autocrlf=true` setting. Keep an
explicit checkout policy for generated artifacts compared byte for byte; do not
normalize their contents during verification. Regenerate with
`pnpm bundle:openapi` if an older working copy retains CRLF. The regression suite
requires Git and exercises fresh disposable checkouts under Windows-style and
LF-preserving settings. Both the repository-wide LF default and a narrower
artifact-specific LF rule are supported. A fixture-local attribute override
lets only the synthetic control file follow `core.autocrlf`; the bundle keeps
the copied repository attributes unchanged. This proves conversion is active
without assuming unrelated files use CRLF. Missing bundle LF protection and
noncanonical bundle bytes remain failing cases. The `text=auto` setting still
detects binary files; the LF policy applies to files Git identifies as text.

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

Baseline.5 adds actor provisioning, lifecycle administration, the actor catalogue,
and lifecycle audit under /api/v1/admin. Every operation requires exact current
platform identity.manage; browser mutations also require exact origin and CSRF
protection. See the [operating procedure](../../docs/operations/actor-administration.md)
for command retry and paging semantics. Existing operation IDs and payloads remain
unchanged.

Baseline.6 adds explicit attach/detach commands, actor-scoped current mapping pages,
and immutable external-identity audit under exact identity.manage authority.
[The operating procedure](../../docs/operations/external-identity-administration.md)
explains replacement, first-receipt retry, paired cursors, and subject-safe logging.
Existing operation IDs and payloads remain unchanged.

## Platform-access administration

Baseline.7 adds exact platform grant/revoke commands, actor-scoped current grants,
and immutable platform-access audit under fresh platform.access.manage authority.
See [the operating procedure](../../docs/operations/platform-access-administration.md)
for delegation, pagination and uncertain self-revocation recovery. Tenant grants
and administration UI remain separate. Existing baseline.6 payloads are retained.

## Tenant administration

Baseline.8 adds tenant provisioning under tenant.provision and catalogue, lifecycle
and lifecycle audit under tenant.lifecycle.manage. Browser mutations reuse the
exact-origin/CSRF boundary. Provisioning is atomic with initial grants; retries
retain the original result without undoing subsequent administration.
See [the operating procedure](../../docs/operations/tenant-administration.md).
Existing baseline.7 paths and payloads remain unchanged.

Tenant-access administration adds three paths in baseline.9. All require fresh
exact tenant access.manage; platform authority never substitutes. Current grants,
atomic grant/revoke and immutable audit remain distinct. See the
[operator procedure](../../docs/operations/tenant-access-administration.md)
for retries, pagination, browser guards and final-administrator self-revocation.

Baseline.10 adds immutable definition publication and bounded collection/version
catalogues. Existing exact reads remain unchanged. See [semantic commissioning](../../docs/operations/semantic-definitions.md) for separate read/write permissions, content-checked retries and audit limits.

Baseline.14 adds protected revisioned device/channel protocol commissioning.
GET inspects current configuration; PUT receipts may resolve historical exact
retries and never roll back the head. See [route commissioning](../../docs/operations/protocol-route-commissioning.md).

## Ingestion diagnostics baseline

Baseline.15 adds `getDeviceIngestionDiagnostics` under exact tenant `protocol.manage`. It returns bounded durable processing evidence and sampled source identities; worker/connection liveness remain `not_observed`. See the [operator and measurement procedure](../../docs/operations/ingestion-diagnostics.md).
