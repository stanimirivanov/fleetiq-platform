# ADR 0113: Activate revisioned device protocol routes

## Status

Proposed, 25 October 2026.

Supersedes: none. Superseded by: none.

## Context

A stored, admitted component must not automatically process traffic for a device.
Guest identity claims are untrusted, issuer-scoped evidence. Commissioning races,
ambiguous command outcomes and live configuration changes require explicit host
selection, immutable attribution and a defined validation boundary.

## Decision

Activate an exact admitted package/world for a registered communication device
and host-configured channel. Explicit tenant `protocol.manage` authorizes GET/PUT
commissioning; ordinary metadata access is insufficient. Retirement is per route,
preserves artifacts/history and does not globally retire a release.

The ingestion capability owns routing values, services and its consumed port.
PostgreSQL implements revision compare-and-swap, immutable history and one-statement
current lookup. The server owns transport DTOs, fresh authorization and browser
mutation protection. Promote postgres's existing SDK dependency from test-only
to normal ownership because the consumed lookup port accepts its bounded,
redacted identity claim; no Wasmtime or new third-party dependency is added.

A head row serializes each tenant/device/channel. Its generation advances exactly
once and references immutable history through a deferred foreign key. A short
transaction locks the tenant in SHARE mode before the head, ordering writes with
suspension. Stored bytes are rehashed before commissioning, and admission foreign
keys pin exact release/world/policy evidence. A stale write conflicts; an exact
same-actor/content retry returns its original generation/receipt even after later
edits, without restoring the old head. GET returns current configuration.

`RegisteredIdentityChannel` is explicit host-selected identifier-only routing
policy, never guest/browser input. Lookup is constrained by tenant, channel,
identifier issuer, exact decoder and world, registered communication capability,
current-policy admission, stored bytes and current tenant eligibility. It yields
routing context, not an authenticated-peer credential. Cryptographic device
identity and enabled listeners belong to transport commissioning.

Pin the route generation and content identity in an opaque application handoff.
Before every future bounded transport turn, revalidate its complete snapshot.
Changed/retired routes, current suspension or changed identifier ownership deny
validation. On any denial or infrastructure failure the transport must close and
permanently discard the handoff. Validation linearizes at one database statement;
already-started bounded work may complete after a concurrent change.

## Consequences

Re-selecting identical content creates a new generation and invalidates older
handoffs. Revisions use the explicit unsigned-32-bit range; exhaustion and
recorded-time regression conflict rather than wrap or rewrite evidence. PUT
receipts may be historical, so operators refresh GET before further edits.

This slice does not open sockets, execute guests, authenticate peers, maintain a
tenant lifecycle epoch or push connection-cancellation events. Suspension denies
validation while inactive; the eventual transport owns prompt closure and the
channel-configuration lifecycle. It cannot infer identity assurance from an IMEI
or serial number. Deletion/GC, a global package kill switch and D06 remain deferred.

## Alternatives considered

- Activate every admitted release: rejected because compilation evidence is not
  deployment or device authority.
- Discover tenants or issuers from claims: rejected because it creates ambiguous
  routing and a cross-tenant identity oracle.
- Overwrite a mutable device/package row: rejected because generations, exact
  retry receipts and configuration history would be lost.
- Introduce a routing service or cache: deferred; the current bounded indexed
  PostgreSQL lookup provides one authoritative eligibility snapshot.
