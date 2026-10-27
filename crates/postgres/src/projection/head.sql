-- Read in the same repeatable view as existence and current source hydration.
SELECT COALESCE((SELECT generation FROM projection.tenant_heads WHERE tenant_id=$1),0::bigint) AS "generation!",
       COALESCE((SELECT revision FROM projection.tenant_heads WHERE tenant_id=$1),0::bigint) AS "revision!",
       COALESCE((SELECT processed_count FROM projection.tenant_heads WHERE tenant_id=$1),0::bigint) AS "processed_count!",
       EXISTS (SELECT 1 FROM projection.pending_changes WHERE tenant_id=$1) AS "pending!"
