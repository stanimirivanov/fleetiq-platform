-- Bounded operational reads for one tenant-owned communication device.
-- The ordered index lets diagnostics seek directly to the newest 65 receipts
-- rather than sorting or aggregating an unbounded accepted-message history.
-- message_id is the deterministic tie-breaker for equal receipt timestamps.
-- No payload/state/value column is indexed, copied, rewritten or exposed.
-- This transactional index build takes PostgreSQL's normal writer-blocking
-- lock; schedule a maintenance window for an already large deployed log.
CREATE INDEX ix_accepted_messages_device_recent
    ON ingestion.accepted_messages
    (tenant_id, device_asset_id, received_at_ms DESC, message_id DESC);
COMMENT ON INDEX ingestion.ix_accepted_messages_device_recent IS
    'Tenant/device recent receipt seek for fixed 64-frame diagnostics plus one coverage sentinel; receipt time is not connection or worker liveness.';
