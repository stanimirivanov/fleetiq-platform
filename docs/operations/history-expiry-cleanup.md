# History expiry cleanup

## TL;DR

Schedule `fleetiq-server expire-history-once` for each tenant that needs
cleanup independently of restore or export traffic. Set
`FLEETIQ_HISTORY_CLEANUP_TENANT_ID` and provide `DATABASE_URL` through the
trusted job's database secret. The command expects a database with the
platform migrations already applied.

## Scheduling and results

Use an external scheduler and run one invocation per configured tenant.
The command exits after one bounded pass. It writes a line such as
`history cleanup restores=1 exports=16 export_leases=2` after both batches
succeed. Each count is capped at 16. A count at the cap warrants another
pass. Later passes can also pick up work that was locked or ineligible
during the preceding invocation.

The restore and export batches commit independently. On failure, retry the
same tenant. Retries skip completed cleanup and preserve live lease fences.
Restore and export receipts remain available under their existing rules.

## Storage accounting

Cleanup clears logical payload bytes from PostgreSQL. Physical disk usage
depends on PostgreSQL vacuum and storage management. Published archive
manifests and their private object-storage bytes are outside this command's
scope.
