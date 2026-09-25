# ADR 0046: Map external identities to internal actors by exact authority and subject

## Status

Accepted, 25 September 2026.

Supersedes: none. Superseded by: none.

## Context

FleetIQ authorization uses a stable internal `ActorId`, while authentication is
expected to be performed by an external identity provider or trusted workload
mechanism. The application cannot safely use a provider subject alone because
subjects are unique only within their issuer or authentication authority. It
also cannot make a token claim, HTTP header, or caller-selected actor ID proof
of identity.

The production provider and credential format have not been selected. The
mapping must therefore support human and workload identities without coupling
the application to OpenID Connect, one issuer URL format, token validation, or
provider roles.

## Decision

FleetIQ identifies a trusted authentication configuration with a validated
`IdentityAuthorityId`. After that configured adapter authenticates a caller, it
produces the authority ID and the authority-local opaque subject as an
`ExternalIdentity`. The subject is case-sensitive, is not normalized, excludes
empty and control-character values, and is bounded to 512 Unicode scalar
values.

PostgreSQL stores an exact mapping keyed by `(authority_id, subject)` to one
existing internal actor. One external identity can map to only one actor. One
actor may have multiple external identities to support provider migration,
separate workforce and workload authorities, or deliberate account linking.
Mapping existence grants no tenant access; the resolved actor must still pass
the existing exact tenant-action policy for every protected operation.

The application owns an `ActorIdentityResolver` port returning either one
internal actor, absence, or an operational failure. Absence must fail
authentication without exposing nearby mappings. Token, certificate, session,
signature, audience, issuer, expiry, nonce, and key validation stay in the
authentication adapter and occur before resolution.

Mapping administration, account-linking proof, deactivation, audit, and API
status mapping require separate use cases before mappings can be changed through
the product.

## Consequences

Business routes can later translate a successfully authenticated external
principal into the stable actor used by provider-neutral authorization. Exact
authority scoping prevents equal subjects from different providers from
colliding, and exact subject comparison avoids silently merging principals.

The database becomes a security-sensitive identity link. Operational database
access must be restricted, and future mapping changes need authenticated,
authorized, auditable administration. Provider configuration must assign stable
authority IDs and must never accept an authority ID selected by an untrusted
request.

This change is additive. Rollback requires removing dependent mappings before
dropping the table. No existing actors or grants require backfill; until a
mapping is provisioned, that external principal does not resolve.

## Alternatives considered

Using the provider subject alone was rejected because equal subjects from
different authorities are unrelated principals.

Using raw issuer URLs as the only authority identity was rejected because not
all authentication mechanisms use issuer URLs and provider endpoint changes
should not silently change internal principal identity.

Using an actor ID from a token or request header directly was rejected because
caller possession of an identifier is not authentication evidence and would
bypass the trusted mapping boundary.

Creating actors automatically on first login was rejected because tenant
onboarding, account linking, deactivation, and audit policy have not been
defined. Resolution therefore returns absence for an unprovisioned principal.
