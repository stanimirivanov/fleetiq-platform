-- Bounded event-time evaluation pages, tenant/asset scoped. The expression
-- matches the immutable window end (evaluation event time), with C-collation
-- evaluation identity as the stable tie-breaker. Existing facts are unchanged.
CREATE INDEX ix_calculator_evaluations_scope_event
    ON analytics.evaluations(tenant_id,scope_asset_id,((replay_key->>'end')::bigint),evaluation_id COLLATE "C");
COMMENT ON INDEX analytics.ix_calculator_evaluations_scope_event IS
    'Supports half-open bounded event-time pages and forward keysets without expanding input/output arrays.';
