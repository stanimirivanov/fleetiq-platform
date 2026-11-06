-- Tenant lock serializes this snapshot with admission, staging, cancellation and policy revision.
SELECT EXISTS (
 SELECT 1 FROM history.work_reservations r JOIN history.lifecycle_policies p
 ON p.tenant_id=r.tenant_id AND p.revision=r.policy_revision
 WHERE r.tenant_id=$1 AND r.reservation_id=$2 AND r.expires_at>clock_timestamp()
 AND p.revision=(SELECT max(revision) FROM history.lifecycle_policies WHERE tenant_id=$1)
 AND p.max_jobs IS NOT NULL
 AND (SELECT COALESCE(sum(reserved_bytes::numeric),0) FROM history.work_reservations WHERE tenant_id=$1 AND expires_at>clock_timestamp())
   + (SELECT COALESCE(sum(octet_length(payload)::numeric),0) FROM history.restore_jobs WHERE tenant_id=$1 AND status='ready' AND expires_at>clock_timestamp()) <= p.max_bytes::numeric
) AS "fits!"
