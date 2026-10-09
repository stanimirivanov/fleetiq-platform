# Browser-safe semantic metadata UI execution plan

## TL;DR

Status: active. Publish the existing semantic metadata reads as a reviewed
backend-owned contract first. Then establish browser-safe identity and tenant
authorization, pin and consume the contract in FleetIQ UX, and expand the
operator guide. The current configured workload bearer never enters browser
code.

## Outcome and boundaries

The approved sample asset catalogue and registry become connected views for
authorized tenant metadata. This plan covers asset identity, pinned semantic
definitions, directed relationships, and exact source-to-property binding
snapshots. It does not promise live property state, geodetic location, alert
lifecycle, whole-graph search, or a production telemetry stream. Those need
separate backend read models and contracts.

## Invariants

- Every protected read uses a current actor and tenant `metadata.read` grant.
  A tenant path segment alone never authorizes a read.
- The browser never receives the existing single-principal workload secret.
  A trusted session or token boundary must map the actual operator to an actor.
- Relationship and binding queries require explicit effective and known
  times. A bounded page and opaque continuation cursor cannot be presented as
  a complete graph or a cross-request snapshot.
- The UX validates unknown wire payloads, distinguishes unavailable and
  unauthorized states, and keeps sample data visibly separate.
- The existing `/api/v1` catalogue meaning remains compatible. Contract
  additions do not silently change persisted meaning or server behavior.

## Ordered pull-request slices

1. **Publish semantic metadata reads (completed, M04).** Add exact
   asset, pinned asset-type/property/relationship-type definitions, asset
   relationship snapshots, and target/source signal-binding snapshots to the
   OpenAPI UI subset. Include response examples, schema and in-process
   compatibility checks, and current-state/client-consumption guidance.
2. **Establish browser-safe identity (completed, M04).** Implement
   provider-neutral OIDC authorization-code login with S256 PKCE, browser-bound
   one-use state, PostgreSQL sessions, and secure host-only cookies. Resolve
   pre-provisioned operators and enforce current actor mappings and tenant
   grants on every protected read. Include origin/CSRF-protected logout,
   expiry cleanup, safe 401/403 responses, and the approved exact dependency
   exception guards. See [ADR 0095](../../adr/0095-authenticate-browser-operators-with-oidc-sessions.md)
   and the [browser identity guide](../../operations/browser-identity.md).
3. **Connect FleetIQ UX metadata views.** Pin an exact merged contract commit,
   regenerate Effect v4 transport code, implement an interruptible validated
   live adapter, and connect catalogue/detail/registry screens to authorized
   reads. Preserve URL filters, selection, temporal cutoffs, pagination,
   explicit empty/error states, and development-only sample previews.
4. **Complete the operator guide.** Cover connected metadata tasks only after
   browser tests prove them; clearly separate any remaining sample-only map,
   signal, and alert workflows.

## Compatibility, rollout, and rollback

The first slice adds documentation of existing `/api/v1` routes without changing
their wire behavior or authentication. Consumers opt in by pinning the new
artifact. The backend must not claim browser readiness merely because a
generated client compiles. Later UX activation is gated on the identity slice
and can be disabled without removing additive v1 contract operations. Once
published to clients, compatible v1 operations remain supported or need an
explicit versioned migration plan.

## Verification and discovery

The contract slice runs repository and architecture checks, schema/example
validation, in-process handler compatibility tests, formatting, and applicable
Rust acceptance checks. The identity slice needs authentication, authorization,
revocation, and cross-tenant negative tests. The UX slice needs production and
development browser journeys, contract validation, and proof that its
production bundle contains no sample payload or workload secret.

The selected strategy is provider-neutral OIDC with the UI and API under one
HTTPS public origin and server-managed sessions. The exact provider is configured
at deployment. Browser identity is implemented and verified with signed-token
fixtures, real PostgreSQL, and HTTP authentication and authorization journeys.
Qualification of a deployed customer provider and HTTPS edge remains separate.
Connected UX and its guide remain outstanding. Never substitute a development
workload-secret proxy for operator authentication.