-- Exact bounded-ID batch, not one catalogue lookup per discovered vertex.
SELECT tenant_id, asset_id, type_id, type_version, display_name
FROM metadata.assets
WHERE tenant_id = $1 AND asset_id = ANY($2::text[])
ORDER BY asset_id COLLATE "C"
LIMIT 100
