# ADR 0070: Extract access policy boundary

## Status

Proposed, 30 September 2026.

Supersedes: none. Superseded by: none.

## Context

Authentication, exact platform and tenant authorization, closed action
vocabulary, and evidence types currently live beside unrelated orchestration
and persistence ports in `fleetiq-application`. Future identity and metadata
capabilities need these contracts without depending on every application use
case. The PostgreSQL resolver and policy evaluators already implement narrow
ports, but importing those ports through the broad application crate hides
their ownership in the dependency graph.

ADR 0047 requires authenticated actor evidence before authorization; ADR 0049
keeps platform actions distinct from tenant actions. Moving the contracts must
preserve their private evidence constructors and current fail-closed behavior.
No persisted or wire representation changes are required.

## Decision

Introduce `fleetiq-access` as a policy crate depending only on `fleetiq-domain`.
It owns external identity resolution and credential-authenticator ports, the
caller authentication service, platform and tenant action vocabularies and
policy ports, and authenticated and authorized evidence. PostgreSQL implements
its resolver and access-policy ports directly. Application orchestration
depends on access and temporarily re-exports the existing public names for
source compatibility. The architecture checker records these direct edges and
rejects access-to-application dependencies.

Keep evidence constructors private. Application unit tests obtain actor
evidence through a fixed test authenticator and resolver rather than opening a
production constructor or a test-only Cargo feature. Existing exact-request,
denial, and operational-error semantics remain unchanged.

## Consequences

Identity and metadata capabilities can depend on access policy without pulling
in broad application orchestration or creating a Cargo cycle. Existing code
using `fleetiq_application` imports continues to compile, while new adapters
and capability crates can import the defining `fleetiq_access` types directly.
The defining crate changes and the workspace gains one package and direct
dependency edges. There is no database migration, deployment change, or
authorization policy change. Rollback is a source-level crate and import
reversal; persisted data is unaffected.

## Alternatives considered

Leaving the contracts in application would preserve broad coupling and make
later identity extraction depend on unrelated use cases. Duplicating evidence
types or action enums would allow divergent security decisions. Making an
evidence constructor public for tests would weaken the trust boundary.
