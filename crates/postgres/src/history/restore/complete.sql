-- Compare the complete decoded identity set, not event time, row count alone or current hot values.
WITH decoded AS (
 SELECT * FROM jsonb_to_recordset($7::jsonb) AS x(message_id TEXT,protocol_package_id TEXT,
 protocol_package_version TEXT,output_index BIGINT,binding_id TEXT,binding_revision NUMERIC,event_time_basis TEXT)
), expected AS (
 SELECT message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis
 FROM history.archive_members WHERE tenant_id=$1 AND job_id=$2
)
UPDATE history.restore_jobs j SET status='ready',payload=$6
FROM history.archive_jobs a,history.work_reservations r
WHERE j.tenant_id=$1 AND j.manifest_id=$2 AND j.request_generation=$3 AND j.attempt=$4 AND j.reservation_id=$5
AND j.status='pending' AND j.lease_until>clock_timestamp() AND j.expires_at>clock_timestamp()
AND a.tenant_id=j.tenant_id AND a.job_id=j.manifest_id AND a.object_sha256 IS NOT NULL
AND octet_length($6::bytea)=a.object_bytes AND sha256($6::bytea)=a.object_sha256
AND r.tenant_id=j.tenant_id AND r.reservation_id=j.reservation_id AND r.expires_at>clock_timestamp()
AND r.reserved_bytes=a.object_bytes
AND r.policy_revision=(SELECT max(revision) FROM history.lifecycle_policies WHERE tenant_id=j.tenant_id)
AND (SELECT count(*) FROM decoded)=(SELECT count(*) FROM expected)
AND NOT EXISTS ((SELECT * FROM decoded EXCEPT SELECT * FROM expected) UNION ALL (SELECT * FROM expected EXCEPT SELECT * FROM decoded))
