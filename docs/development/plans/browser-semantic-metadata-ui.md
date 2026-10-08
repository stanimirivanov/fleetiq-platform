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

1. **Publish semantic metadata reads (this backend PR, M04).** Add exact
   asset, pinned asset-type/property/relationship-type definitions, asset
   relationship snapshots, and target/source signal-binding snapshots to the
   OpenAPI UI subset. Include response examples, schema and in-process
   compatibility checks, and current-state/client-consumption guidance.
2. **Establish browser-safe identity (backend prerequisite).** Select the
   trusted browser deployment and sign-in/session strategy, map each operator
   to an actor, and enforce current tenant grants on every read. Review
   credential handling, origin/CORS or cookie/CSRF policy, revocation,
   unauthorized behavior, and cross-tenant isolation before enabling live UX.
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

The identity provider, hosting topology, and operator session mechanism are
not yet selected. Resolve them before slice 2; do not substitute a
development-only secret-bearing proxy for a production browser identity.