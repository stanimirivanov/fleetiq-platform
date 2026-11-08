# Tenant archive and export storage accounting

## TL;DR

Read committed archive lengths and retained export totals under current tenant
operations access. Export totals separate ready, pending, and expired payloads.

## Read the snapshot

Call `GET /api/v1/tenants/{tenant_id}/storage-usage/lifecycle` with current
tenant `operations.read`. The route uses the existing workload or browser
authentication and returns a single observation time in Unix milliseconds.
Counts and bytes are exact nonnegative decimal strings.

- `archive` counts committed manifests once and sums their recorded object
  lengths. An unpublished claim contributes nothing. This is a PostgreSQL
  manifest inventory, with no request to the storage provider.
- `exports.downloadable` counts ready, unexpired retained payloads.
- `exports.pending` counts pending, unexpired retained payloads.
- `exports.expired_retained` counts payloads past their lifetime that still
  await cleanup, whether they were pending or ready.

Export classes are disjoint at the database observation time. Once cleanup
or cancellation clears a payload, its receipt contributes no records or
bytes. The byte totals use the verified artifact length stored on each job.
A ready artifact still requires current download authority and admission.

The read uses one repeatable-read snapshot with a two-second statement
deadline. Failure returns a redacted error. The figures describe logical
evidence and exclude indexes, shared storage, provider allocation, restore
staging, and billing. The original `storage-usage` endpoint continues to
report accepted messages, decoded observations, and property interpretations.
