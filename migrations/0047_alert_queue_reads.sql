-- Stable tenant/rule-revision keysets for observed alerts. Version-zero
-- configuration checkpoints are not alerts and stay outside this read index.
-- C collation makes traversal independent of database locale. Phase is mutable
-- and deliberately not part of the cursor/order. Transactional index creation
-- can briefly block lifecycle writes; no facts or selections are rewritten.
CREATE INDEX ix_alert_lifecycles_queue
    ON analytics.alert_lifecycles(tenant_id,rule_id COLLATE "C",revision)
    WHERE version>0;
COMMENT ON INDEX analytics.ix_alert_lifecycles_queue IS
    'Observed-alert queue identity order; phase filtering can change during paging, which is traversal rather than a live snapshot.';
