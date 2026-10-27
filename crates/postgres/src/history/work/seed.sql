WITH cursor AS MATERIALIZED (
    SELECT * FROM history.semantic_backfill WHERE NOT completed
    FOR UPDATE SKIP LOCKED
), batch AS MATERIALIZED (
    -- Keep initial and continuing paths separate. An OR here turns
    -- the checkpoint comparison into a join filter rather than an
    -- indexed range, rescanning old keys on every batch.
    SELECT source.* FROM cursor c CROSS JOIN LATERAL (
        (SELECT o.tenant_id,o.message_id,o.protocol_package_id,
                o.protocol_package_version,o.output_index,o.device_recorded_at_ms
         FROM ingestion.decoded_observations o WHERE c.tenant_id IS NULL
         ORDER BY o.tenant_id,o.message_id,o.protocol_package_id,o.protocol_package_version,o.output_index
         LIMIT 32)
        UNION ALL
        (SELECT o.tenant_id,o.message_id,o.protocol_package_id,
                o.protocol_package_version,o.output_index,o.device_recorded_at_ms
         FROM ingestion.decoded_observations o WHERE c.tenant_id IS NOT NULL AND
             (o.tenant_id,o.message_id,o.protocol_package_id,o.protocol_package_version,o.output_index)
             > (c.tenant_id,c.message_id,c.protocol_package_id,c.protocol_package_version,c.output_index)
         ORDER BY o.tenant_id,o.message_id,o.protocol_package_id,o.protocol_package_version,o.output_index
         LIMIT 32)
    ) source
), inserted AS (
    INSERT INTO history.semantic_work
        (tenant_id,message_id,protocol_package_id,protocol_package_version,
         output_index,event_time_basis,known_at_ms)
    SELECT tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,
           CASE WHEN device_recorded_at_ms IS NULL THEN 'received_at' ELSE 'device_recorded' END,
           floor(extract(epoch FROM clock_timestamp())*1000)::bigint
    FROM batch ON CONFLICT DO NOTHING
), last_key AS (
    SELECT * FROM batch ORDER BY tenant_id DESC,message_id DESC,
        protocol_package_id DESC,protocol_package_version DESC,output_index DESC LIMIT 1
)
UPDATE history.semantic_backfill b SET
    tenant_id = COALESCE(k.tenant_id,b.tenant_id),
    message_id = COALESCE(k.message_id,b.message_id),
    protocol_package_id = COALESCE(k.protocol_package_id,b.protocol_package_id),
    protocol_package_version = COALESCE(k.protocol_package_version,b.protocol_package_version),
    output_index = COALESCE(k.output_index,b.output_index),
    completed = k.tenant_id IS NULL
FROM cursor c LEFT JOIN last_key k ON TRUE WHERE b.singleton=c.singleton
