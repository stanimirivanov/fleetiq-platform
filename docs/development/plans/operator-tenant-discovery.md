# Operator tenant discovery execution plan

## TL;DR

Status: implemented; final qualification is recorded in the PR handoff.
Milestone: M02 - Product Foundation.
Deliver plan PR 2: a bounded current-access read for the authenticated caller,
without changing sessions or weakening per-operation authorization.

## Scope

- Access-owned query, results, repository port, and authentication-consuming service.
- One PostgreSQL query with complete action sets and tenant-level lookahead.
- GET /api/v1/me/tenants, typed validation, no-store responses, and redacted errors.
- Modular OpenAPI contract, examples, checked SQL metadata, and operator guidance.

## Boundaries and decisions

See [ADR 0099](../../adr/0099-discover-current-operator-tenant-access.md).
Keep discovery distinct from administrative catalogues and authorization evidence.
No new schema, dependency, cache, frontend behavior, or administration endpoint.

## Verification

Exercise limits, ordering, complete action sets, actor isolation, no grants,
revocation, tenant suspension, actor deactivation, authentication precedence,
invalid queries, unknown actor selectors, error redaction, and browser sessions.
Prove a discovery result cannot authorize a later metadata read after revocation.
Run focused tests, OpenAPI verification, repository/architecture sensors,
SQLx metadata regeneration, and the full online Rust acceptance gate.

## Progress and limitations

Implementation and final check results are reported in the task handoff.
A real customer OIDC provider and deployed HTTPS edge remain outside this slice.
