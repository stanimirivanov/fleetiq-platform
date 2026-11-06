-- Atomic coverage/file observation; never read object storage or current hot payloads.
WITH snapshot AS (
 SELECT CASE WHEN r.manifest_id IS NULL THEN 'archived'
 WHEN r.request_generation<>$3 OR r.expires_at<=clock_timestamp() OR r.status IN ('cancelled','expired') THEN 'unavailable'
 WHEN r.status='pending' THEN 'restoring'
 WHEN r.status='ready' AND octet_length(r.payload)=a.object_bytes AND sha256(r.payload)=a.object_sha256 THEN 'restored'
 ELSE 'unavailable' END AS state,r.payload
 FROM history.archive_jobs a JOIN metadata.tenants t USING (tenant_id)
 LEFT JOIN history.restore_jobs r ON r.tenant_id=a.tenant_id AND r.manifest_id=a.job_id
 WHERE a.tenant_id=$1 AND a.job_id=$2 AND a.object_sha256 IS NOT NULL AND t.is_active
)
SELECT state AS "state!",CASE WHEN state='restored' THEN payload ELSE NULL END AS payload FROM snapshot
