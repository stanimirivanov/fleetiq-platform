-- Indexed stable identity sample, not a claim to enumerate every eligible tenant row.
WITH sample AS (
 SELECT * FROM history.asset_property_changes WHERE tenant_id=$1
 ORDER BY message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis
 LIMIT 101
)
SELECT (h.event_at_ms IS NOT NULL) AS "payload_present!",
 EXISTS(SELECT 1 FROM projection.current_properties c LEFT JOIN history.archive_members pin
 USING (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis)
 WHERE c.tenant_id=h.tenant_id AND
 ((c.message_id,c.protocol_package_id,c.protocol_package_version,c.output_index,c.binding_id,c.binding_revision,c.event_time_basis)
 =(h.message_id,h.protocol_package_id,h.protocol_package_version,h.output_index,h.binding_id,h.binding_revision,h.event_time_basis)
 OR pin.job_id=m.job_id)) AS "current_projection!",
 EXISTS(SELECT 1 FROM analytics.evaluations e CROSS JOIN LATERAL jsonb_array_elements(e.replay_key->'inputs') input
 LEFT JOIN history.archive_members pin ON pin.tenant_id=e.tenant_id
 AND analytics.semantic_evidence_id(pin.tenant_id,pin.message_id,pin.protocol_package_id,pin.protocol_package_version,pin.output_index,pin.binding_id,pin.binding_revision,pin.event_time_basis)=input->>0
 WHERE e.tenant_id=h.tenant_id AND (pin.job_id=m.job_id
 OR input->>0=analytics.semantic_evidence_id(h.tenant_id,h.message_id,h.protocol_package_id,h.protocol_package_version,h.output_index,h.binding_id,h.binding_revision,h.event_time_basis))) AS "pinned_analytics!",
 analytics.semantic_evidence_id(h.tenant_id,h.message_id,h.protocol_package_id,h.protocol_package_version,h.output_index,h.binding_id,h.binding_revision,h.event_time_basis) AS "id!",
 CASE WHEN a.object_sha256 IS NOT NULL AND a.generation>0 AND a.lease_until IS NOT NULL
 THEN floor(extract(epoch FROM a.lease_until)*1000)::bigint ELSE NULL END AS age_floor,
 (a.object_sha256 IS NOT NULL AND a.generation>0 AND a.lease_until IS NOT NULL) AS "verified!"
FROM sample h LEFT JOIN history.archive_members m USING (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis)
LEFT JOIN history.archive_jobs a ON a.tenant_id=m.tenant_id AND a.job_id=m.job_id
ORDER BY h.message_id,h.protocol_package_id,h.protocol_package_version,h.output_index,h.binding_id,h.binding_revision,h.event_time_basis
