# Protocol package storage and admission

## TL;DR

Register exact bytes, then explicitly admit one WIT world. Neither operation
activates a device or grants a session authority. The maintained admission
worker requires Linux and is included beside the backend in its runtime image.
Existing current-policy evidence can be returned without recompilation.

## Prerequisites

- A migrated database and authenticated active tenant/operator. The runtime
  database role needs SELECT/INSERT on the new ingestion artifact/admission
  tables and its existing catalogue/identity permissions; grant no DDL authority.
- Clients using closed action enums regenerated against HTTP baseline.13 before
  delegating the new action; earlier clients may reject discovery/grant responses
  containing `protocol.manage`. Existing grants are not automatically expanded.
- Exact current tenant `protocol.manage`, delegated through the audited tenant
  grant API by an `access.manage` operator. No existing metadata grant implies it.
- A built import-free protocol component; see the [guest guide](../../examples/custom-protocol/README.md).
- For admission, install `fleetiq-server` and `fleetiq-protocol-admission` from
  the same reviewed build in one directory. Never substitute a downloaded worker.
  The production Dockerfile builds and packages both.

## Register bytes

`PUT /api/v1/tenants/{tenant_id}/protocol-packages/{package_id}/versions/{package_version}`

Send the nonempty raw component (maximum 8 MiB) with exact
`Content-Type: application/wasm` and `X-Artifact-Sha256` containing its lowercase
64-character SHA-256. Browser mutations also require the configured exact Origin
and `X-FleetIQ-CSRF: 1`; workload clients use their configured bearer credential.
Do not put credentials into issue reports, committed scripts or example URLs.

A first catalogue registration plus stored content returns 201. Exact retries
return 200. Digest mismatch returns 400; release-key/content collision returns
409 without replacement. A failed storage operation may leave catalogue-only
metadata; retry identical bytes with the same key. GET on the same route returns
only a fully stored release identity, never code bytes or metadata-only success.

## Admit explicitly

`POST /api/v1/tenants/{tenant_id}/protocol-packages/{package_id}/versions/{package_version}/admissions`

Send closed JSON `{"world":"runtime-v1"}` or `{"world":"identity-v1"}` with
`Content-Type: application/json`. Identity support must be chosen explicitly.
The request body is limited to 1 KiB. Success returns the immutable release,
world, policy version, original actor and first successful admission time.
Retries retain that evidence and do not recompile under the same policy.

Rejection returns 422. Occupied compilation capacity returns 429. Missing or
unsupported isolation, worker failure or timeout returns 503; none creates a
successful receipt. Retry deliberately after resolving the stated prerequisite.
Every protected response is no-store and omits raw package/compiler diagnostics.

## Resource and recovery boundary

Policy 1 allows one compiler per backend process, 15 seconds wall time, 10 CPU
seconds and 2 GiB virtual-address space in the Linux child. Core dumps are off.
The child gets no application environment and no guest execution authority.
Cancellation keeps the slot occupied until the supervisor terminates/reaps the
child. There is no native-code cache, automatic retry or activation side effect.

A successful compile followed by an uncertain database commit is retried using
the same release/world; first durable evidence wins. Existing admission is valid
only for its exact policy version. Keep artifact rows/evidence in database
backup/restore. Retention requires a reviewed future migration; do not delete
immutable evidence to force re-admission. Container/host capacity and kernel
isolation remain separate deployment responsibilities.

See [ADR 0112](../adr/0112-store-and-isolate-protocol-admission.md) for trade-offs
and the [HTTP contract](../../contracts/http/README.md) for generated client input.

Raw artifacts are addressed by `(tenant, SHA-256)`; identical releases share
bytes only within one tenant. Admission foreign keys bind both the exact
release and its digest, independently of the selected world/policy key.
