# Publish and discover semantic definitions

## TL;DR

Publish immutable asset types, observable properties and directed relationship
types with current metadata.manage in an active tenant. Discover their versions
with separate metadata.read authority. Changing meaning always creates a new
version; existing assets and bindings keep their pinned references.

## Before publishing

Use the configured [browser identity](browser-identity.md) or separate workload
listener and explicitly select an accessible tenant. A platform grant or tenant
access.manage does not imply metadata.manage or metadata.read. Delegate exact
tenant actions through [access administration](tenant-access-administration.md).

Browser POST requires the session cookie, one configured Origin and one
X-FleetIQ-CSRF header with value 1. Bearer credentials cannot bypass browser mode.
Request bodies cannot supply tenant, administrator or trusted receipt time.

## Publish complete immutable content

POST the following shapes under /api/v1/tenants/{tenant_id}:

~~~text
/properties
{"id":"temperature","version":1,"name":"Temperature",
 "value_kind":"decimal","canonical_unit":"celsius"}

/asset-types
{"id":"equipment","version":1,"name":"Equipment",
 "supported_properties":[{"id":"temperature","version":1}]}

/relationship-types
{"id":"physical.contains","version":1,"name":"Contains"}
~~~

Publish property versions before asset types that reference them. Property
references resolve exactly in the same tenant. Their declared order is preserved.
Repeated property IDs are invalid even when versions differ. Asset types may
declare no properties, or up to 100. JSON is limited to 16 KiB.

Versions range from 1 through 4294967295 and may have gaps. Names are bounded to
200 Unicode scalar values; identifiers and units to 128. Leading/trailing
whitespace, controls and unknown fields are invalid. Property value_kind is
boolean, integer, decimal or text. Only numeric kinds may have a canonical unit.
Omitting canonical_unit means null. Publication performs no unit conversion or
automatic version selection.

HTTP 201 with outcome published means the complete version committed. HTTP 200
with outcome already_published means identical content already exists, including
after a lost response. Reuse the same key and content on retry. A different name,
value kind, unit, property membership or property order under that version returns
409. A semantic change needs a new version, never overwrite or delete.

The result is based on immutable content, not publisher identity. Another
currently authorized publisher can resolve identical content. This capability
does not record publisher/time audit or backfill provenance for older strict
writes. Immutable semantic history is distinct from attributed administration
audit; deployment requirements for that additional audit remain separate.

## Discover definitions and versions

GET each collection returns definitions (ID, version, name) and next_after.
Rows are ordered by case-sensitive ID, then numeric version ascending. One ID
with versions 1, 2 and 10 returns that order, not lexical 1, 10, 2. A collection
contains versions rather than one latest row per definition.

Use limit (1..100, default 50). If next_after is present, send both after_id and
after_version. The cursor may name a position that does not currently exist.
Do not infer a total count or a complete registry from one page.

For versions of one exact identity, use:

~~~text
GET /api/v1/tenants/{tenant_id}/properties/{property_id}/versions
GET /api/v1/tenants/{tenant_id}/asset-types/{type_id}/versions
GET /api/v1/tenants/{tenant_id}/relationship-types/{type_id}/versions
~~~

These accept the same cursor and limit. after_id must equal the routed identity.
A missing identity returns an empty terminal page. Resolve complete content
through the existing versions/{version} exact GET. The meaning and payloads of
those exact reads are unchanged.

Cursor traversal is not a cross-request snapshot. Newly published versions can
appear after your cursor; values inserted before it require a fresh traversal.
Every page needs current metadata.read. A publication grant alone cannot read
the catalogue, and a read grant alone cannot publish.

## Failures, security and rollback

All outcomes are no-store and problems hide credentials, SQL and constraint
details. Invalid bodies, cursors and limits return 400; authentication failures
401; missing exact authority or failed browser mutation guards 403. A tenant
that cannot be authorized returns 403. A missing referenced property version
returns 404 without a partial parent; changed occupied content returns 409;
infrastructure failure returns 500. Unsupported methods return 405 with Allow.

Suspension and revocation deny subsequent decisions, including publication
retries; in-flight work and already issued evidence are not recalled. Definitions
remain stored. Existing strict Rust insert calls continue to reject duplicates.

No migration, dependency or runtime setting changes are needed. Rollback removes
these additive routes while retaining definitions and existing exact reads.
The [HTTP contract](../../contracts/http/openapi/openapi.yaml) is authoritative
for payloads. Operator UI editors and production provider qualification are later work.
