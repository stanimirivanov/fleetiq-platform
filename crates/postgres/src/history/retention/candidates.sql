-- Indexed stable identity sample, not a claim to enumerate every eligible tenant row.
WITH sample AS (
 SELECT * FROM history.asset_property_changes WHERE tenant_id=$1
 ORDER BY message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis
 LIMIT 101
)
SELECT analytics.semantic_evidence_id(h.tenant_id,h.message_id,h.protocol_package_id,h.protocol_package_version,h.output_index,h.binding_id,h.binding_revision,h.event_time_basis) AS "id!",
 CASE WHEN a.object_sha256 IS NOT NULL AND a.generation>0 AND a.lease_until IS NOT NULL
 THEN floor(extract(epoch FROM a.lease_until)*1000)::bigint ELSE NULL END AS age_floor,
 (a.object_sha256 IS NOT NULL AND a.generation>0 AND a.lease_until IS NOT NULL) AS "verified!"
FROM sample h LEFT JOIN history.archive_members m USING (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis)
LEFT JOIN history.archive_jobs a ON a.tenant_id=m.tenant_id AND a.job_id=m.job_id
ORDER BY h.message_id,h.protocol_package_id,h.protocol_package_version,h.output_index,h.binding_id,h.binding_revision,h.event_time_basis
