-- A repeated source signal at the same packet/device time cannot identify one
-- record. Reject it rather than pairing independently selected axes across fixes.
-- Current candidates are page-bounded; EXISTS stops at the first conflicting row.
SELECT DISTINCT c.asset_id
FROM projection.current_properties c
JOIN ingestion.decoded_observations o ON o.tenant_id=c.tenant_id AND o.message_id=c.message_id AND o.protocol_package_id=c.protocol_package_id AND o.protocol_package_version=c.protocol_package_version AND o.output_index=c.output_index
WHERE c.tenant_id=$1 AND c.asset_id=ANY($2::text[])
 AND ((c.property_id=$3 AND c.property_version=$4) OR (c.property_id=$5 AND c.property_version=$6))
 AND EXISTS (SELECT 1 FROM ingestion.decoded_observations d
   WHERE d.tenant_id=o.tenant_id AND d.message_id=o.message_id AND d.protocol_package_id=o.protocol_package_id AND d.protocol_package_version=o.protocol_package_version
     AND d.endpoint_id=o.endpoint_id AND d.signal_id=o.signal_id AND d.device_recorded_at_ms IS NOT DISTINCT FROM o.device_recorded_at_ms AND d.output_index<>o.output_index)
