-- Holds deliberately cover the tenant: narrower dependency reachability is not yet proven.
SELECT floor(extract(epoch FROM transaction_timestamp())*1000)::bigint AS "observed!",
 COALESCE(p.revision,0)::bigint AS "revision!",p.hot_days,
 EXISTS(SELECT 1 FROM ingestion.replay_work WHERE tenant_id=$1 AND status<>'completed') AS "replay!",
 (NOT EXISTS(SELECT 1 FROM history.semantic_backfill WHERE completed) OR EXISTS(SELECT 1 FROM history.semantic_work WHERE tenant_id=$1 AND status<>'completed')) AS "semantic!",
 (EXISTS(SELECT 1 FROM analytics.calculator_schedules s LEFT JOIN analytics.calculator_configurations c ON c.tenant_id=s.tenant_id AND c.configuration_id=s.configuration_id WHERE s.tenant_id=$1 AND (c.configuration_id IS NULL OR c.active_revision IS NOT NULL))
 OR EXISTS(SELECT 1 FROM analytics.calculator_work WHERE tenant_id=$1 AND status<>'done')
 OR EXISTS(SELECT 1 FROM analytics.prepared_evaluations p WHERE p.tenant_id=$1 AND NOT EXISTS
   (SELECT 1 FROM analytics.evaluations e WHERE e.tenant_id=p.tenant_id AND e.evaluation_id=p.evaluation_id))) AS "calculator!",
 EXISTS(SELECT 1 FROM history.restore_jobs WHERE tenant_id=$1 AND
  ((status IN ('pending','ready') AND expires_at>transaction_timestamp()) OR lease_until>transaction_timestamp())) AS "restore!"
 ,EXISTS(SELECT 1 FROM projection.pending_changes WHERE tenant_id=$1) AS "projection!"
FROM metadata.tenants t
LEFT JOIN LATERAL (SELECT revision,hot_days FROM history.lifecycle_policies WHERE tenant_id=t.tenant_id ORDER BY revision DESC LIMIT 1) p ON true
WHERE t.tenant_id=$1 AND t.is_active
