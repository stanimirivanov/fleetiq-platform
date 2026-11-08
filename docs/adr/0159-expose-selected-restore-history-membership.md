# ADR 0159: Expose selected restore history membership

## Status

Proposed, 8 November 2026.

## Context

A combined property history page can contain both hot and restored rows.
Clients need to choose the exact provenance lookup for each row.

## Decision

Baseline.39 adds opt-in page-local membership indices to combined history.
Derive membership from the verified restore snapshot used to build the page.
Mark matching identities after ordering, deduplication, and pagination.
Keep provenance authorization and restore validation at the existing endpoint.

## Consequences

Clients that omit the option retain the existing response shape and cursor
semantics. Clients that use the option need a backend with baseline.39.
Membership describes the snapshot used for that page.

## Alternatives considered

Repeating opaque interpretation tokens would increase response size. Emitting
membership unconditionally would affect clients that validate the closed
legacy schema.

## Verification

PostgreSQL route tests cover mixed, deduplicated, continued, and expired
pages. Contract validation checks the published response schema.
