# ADR 0109: Separate production serving from schema migration

## Status

Proposed, 17 October 2026.

Implements Kubernetes delivery-plan D01, issue #302;
acceptance awaits human review.

Supersedes: none. Superseded by: none.

## Context

The development process and bootstrap command apply migrations during startup.
A Kubernetes release must apply its pinned schema once under a separate role,
then run ordinary workloads without DDL authority. Private provider PKI also
needs explicit additional OIDC trust without disabling certificate verification.

## Decision

Add migration-only and inventory commands. Production manifests select
`serve --check-schema` and `bootstrap-platform-operator --check-schema`.
Keep no-argument startup, `serve` and unqualified bootstrap compatible with
existing development composition. Their ability to migrate is development-only
and must never be selected by future production packages.

The PostgreSQL adapter reads the schema-qualified SQLx ledger in one bounded
snapshot. Its exact version/checksum set must equal the embedded migration set;
missing, failed, modified, inaccessible and unknown/newer history fail closed.
Readiness repeats this validation. Validation performs no CREATE, migration lock,
ledger repair or DDL. It is not a fence against concurrently migrating writers.

The explicit migration command reads only DATABASE_URL, retains SQLx's advisory
locking and transactional migration semantics, checks the final set and closes
its sole pooled connection on failure as well as success. The credential-free
JSON inventory records a format identifier, ordered decimal-text versions, descriptions and
SHA-384 checksums (text preserves signed-64-bit precision in JSON consumers); producer image provenance must bind it to an immutable image
and source revision. It does not attest a database or authorize a release.

Migration sessions explicitly select public for ledger/extension resolution and
close that connection rather than leak its settings or locks into the pool.
A shadow ledger in a role-specific schema is never adopted.

Production migration-owner, runtime DML and bootstrap DML authority are distinct.
The checked bootstrap still creates the existing one-time atomic identity/grant
facts and consumes no HTTP identity configuration. It does not run DDL or give
the first web visitor authority. Exact retries retain their existing meaning.

An optional mounted PEM CA bundle adds roots to the server-only OIDC client.
Read at most 256 KiB and 32 certificate blocks; refuse absent, empty, malformed,
mixed or oversized content. The maintained TLS library parses certificates;
normal roots, HTTPS/hostname verification, redirect refusal, timeouts, response
bounds and issuer/claim checks stay enabled. Discovery, JWKS refresh and token
exchange reuse that client. Configuration errors never render bundle paths,
contents, credentials or upstream responses. Rotation requires restart.

## Consequences

A production process refuses stale schema before binding HTTP; later ledger
mismatch makes readiness fail. Liveness remains independent. Exact matching
also refuses an older binary after an upgrade, including additive migrations.
Rolling compatibility needs a future reviewed expand/contract policy; reverting
a Git ref cannot reverse schema changes. The ledger cannot prove an administrator
has not manually altered other database objects.

Release orchestration must fence all writers and independently approve migration.
Graceful stop drains accepted HTTP requests, joins browser cleanup, then closes
the pool; an external supervisor owns the termination deadline and crash recovery.
The writer inventory presently includes HTTP commands/login/session changes and
expired browser-record cleanup; future ingestion/workers must join it explicitly.

Dev-only rcgen and Tokio Rustls generate ephemeral certificates and serve real
loopback HTTPS fixtures. No static test key, extra production TLS stack or
hand-written certificate verifier is introduced. serde_json moves to the server's
normal dependencies for actual inventory output. All new ownership edges remain
inside the composition root's test/runtime boundary.

## Alternatives considered

Running migrations in every Pod needs DDL privileges and hides release ordering.
Checking only the newest version misses gaps and changed bytes. Calling SQLx's
migration runner as a check can create its ledger and execute DDL. Accepting
unknown newer history asserts unproven rollback compatibility. Replacing default
roots or disabling TLS checks for a local provider weakens identity trust.
Committed fixture private keys require secret exceptions and can expire; generated
fixtures prove the same protocol boundary with no deployment key material.
