# Actor provisioning and lifecycle HTTP slice

Status: Implemented and verified.
Milestone: M02 - Product Foundation.

## TL;DR

Expose existing identity use cases through four protected HTTP operations, with
shared browser mutation protection and durable retries after lost responses.
Keep domain/application behavior out of handlers.

## Scope and sequence

1. Compose exact current identity.manage policy, actor provisioning, lifecycle,
   actor catalogue, and lifecycle audit on the configured authenticated listener.
2. Share exact-origin and CSRF-marker protection with logout. Select the policy
   from listener configuration and retain mutually exclusive credential modes.
3. Add explicit first-receipt delivery policy while preserving strict timestamps
   for existing immutable command callers. Reuse existing transaction algorithms.
4. Publish modular OpenAPI, examples, safe problem statuses, and operating guidance.
5. Verify real PostgreSQL concurrency, rollback, pagination, deactivation, lockout,
   wrong-action denial, input bounds, and browser CSRF/session behavior.

## Boundaries and exclusions

The server owns HTTP DTOs and composition; identity owns commands and retry policy;
PostgreSQL owns transactions. No new dependencies, migrations, provider user
creation, invitations, external-identity replacement, grants, tenants, or UI.

## Verification and recovery

Run the complete online acceptance gate and the OpenAPI verification group from
[the contributor agreement](../../../CONTRIBUTING.md#verification).
Tests additionally submit different host receipt times to concurrent deliveries,
confirm the original durable time is retained, and confirm strict commands still
reject a different time. Existing transaction and recovery-authority tests remain
required. Rollback removes additive routes and retains durable identity data.

See [ADR 0100](../../adr/0100-expose-actor-administration-over-http.md) and the
[operating procedure](../../operations/actor-administration.md).

## Dependency qualification

The original acceptance run rejected rustls 0.23.45 with RUSTSEC-2026-0346,
RUSTSEC-2026-0347, RUSTSEC-2026-0348, and RUSTSEC-2026-0349.
The follow-up [security patch review](rustls-security-update.md) records the
minimal update to rustls 0.23.46 and passing supply-chain checks. No advisory
exception or security policy was changed. The complete online acceptance gate
now passes after this qualification.
