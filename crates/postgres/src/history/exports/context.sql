-- Distinguish disabled tenant, impossible target and failed admission before source reads.
SELECT t.is_active AS "active!",EXISTS(SELECT 1 FROM metadata.asset_type_properties supported WHERE supported.tenant_id=a.tenant_id AND supported.type_id=a.type_id AND supported.type_version=a.type_version AND supported.property_id=$3 AND supported.property_version=$4) AS "target_present!",
 EXISTS(SELECT 1 FROM history.work_reservations r JOIN history.lifecycle_policies p
 ON p.tenant_id=r.tenant_id AND p.revision=r.policy_revision
 WHERE r.tenant_id=t.tenant_id AND r.reservation_id=$6 AND r.reserved_bytes>=4194304
 AND r.expires_at>clock_timestamp() AND p.max_jobs IS NOT NULL
 AND p.revision=(SELECT max(revision) FROM history.lifecycle_policies WHERE tenant_id=t.tenant_id)) AS "admitted!",
 EXISTS(SELECT 1 FROM history.asset_property_changes h JOIN metadata.signal_binding_revisions b
 ON b.tenant_id=h.tenant_id AND b.binding_id=h.binding_id AND b.revision=h.binding_revision
 WHERE h.tenant_id=$1 AND b.target_asset_id=$2 AND b.target_property_id=$3 AND b.target_property_version=$4
 AND h.event_at_ms IS NULL AND NOT EXISTS(
 SELECT 1 FROM history.archive_members m WHERE (m.tenant_id,m.message_id,m.protocol_package_id,m.protocol_package_version,m.output_index,m.binding_id,m.binding_revision,m.event_time_basis)
 =(h.tenant_id,h.message_id,h.protocol_package_id,h.protocol_package_version,h.output_index,h.binding_id,h.binding_revision,h.event_time_basis) AND m.job_id=$5::bigint)) AS "missing!"
FROM metadata.tenants t
LEFT JOIN metadata.assets a ON a.tenant_id=t.tenant_id AND a.asset_id=$2
WHERE t.tenant_id=$1
