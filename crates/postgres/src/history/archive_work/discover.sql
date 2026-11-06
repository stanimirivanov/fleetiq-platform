-- Identity anti-join, never event-time or identity-sequence watermarks. New late commits remain discoverable.
INSERT INTO history.archive_members
(tenant_id,job_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis)
SELECT h.tenant_id,$2,h.message_id,h.protocol_package_id,h.protocol_package_version,h.output_index,h.binding_id,h.binding_revision,h.event_time_basis
FROM history.asset_property_changes h
JOIN ingestion.decoded_observations d USING (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index)
JOIN metadata.signal_binding_revisions b ON b.tenant_id=h.tenant_id AND b.binding_id=h.binding_id AND b.revision=h.binding_revision
WHERE h.tenant_id=$1
AND octet_length(row_to_json(h)::text)+octet_length(row_to_json(d)::text)+octet_length(row_to_json(b)::text)<=32768
AND octet_length(COALESCE(d.value_text,''))<=8192
AND octet_length(COALESCE(d.value_decimal::text,''))<=8192
AND octet_length(COALESCE(d.source_unit,''))<=8192
AND NOT EXISTS (SELECT 1 FROM history.archive_members m WHERE
 (m.tenant_id,m.message_id,m.protocol_package_id,m.protocol_package_version,m.output_index,m.binding_id,m.binding_revision,m.event_time_basis)=
 (h.tenant_id,h.message_id,h.protocol_package_id,h.protocol_package_version,h.output_index,h.binding_id,h.binding_revision,h.event_time_basis))
ORDER BY h.message_id,h.protocol_package_id,h.protocol_package_version,h.output_index,h.binding_id,h.binding_revision,h.event_time_basis
LIMIT 16
