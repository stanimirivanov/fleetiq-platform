-- Discover identities by either endpoint, then select each latest known
-- revision BEFORE effective-time/endpoint filtering. A correction can move an
-- edge away; prefiltering its latest revision would resurrect obsolete facts.
WITH candidates AS (
    SELECT DISTINCT binding_id
    FROM metadata.signal_binding_revisions
    WHERE tenant_id = $1 AND recorded_at_ms <= $4
      AND (source_device_asset_id = ANY($2::text[]) OR target_asset_id = ANY($2::text[]))
      AND NOT (binding_id = ANY($5::text[]))
), known AS (
    SELECT revision.*
    FROM candidates AS candidate
    CROSS JOIN LATERAL (
        SELECT stored.* FROM metadata.signal_binding_revisions AS stored
        WHERE stored.tenant_id = $1 AND stored.binding_id = candidate.binding_id
          AND stored.recorded_at_ms <= $4
        ORDER BY stored.revision DESC LIMIT 1
    ) AS revision
)
SELECT binding_id, revision, recorded_at_ms, source_device_asset_id, source_endpoint_id, source_signal_id, target_asset_id, target_property_id, target_property_version, effective_start_ms, effective_end_ms
FROM known
WHERE (source_device_asset_id = ANY($2::text[]) OR target_asset_id = ANY($2::text[]))
  AND effective_start_ms <= $3 AND (effective_end_ms IS NULL OR $3 < effective_end_ms)
ORDER BY binding_id COLLATE "C"
LIMIT $6
