-- A PostgreSQL row lock requires UPDATE on at least one column. A constant
-- virtual generated column provides that privilege without granting changes to
-- tenant lifecycle, replay checkpoints, or operator-managed rule selection.
-- This is metadata-only on PostgreSQL 18. The short DDL locks require stopped
-- writers, as for other platform ledger changes.
ALTER TABLE metadata.tenants
    ADD COLUMN runtime_lock_guard boolean GENERATED ALWAYS AS (true) VIRTUAL;
ALTER TABLE ingestion.replay_sessions
    ADD COLUMN runtime_lock_guard boolean GENERATED ALWAYS AS (true) VIRTUAL;
ALTER TABLE analytics.calculator_configurations
    ADD COLUMN runtime_lock_guard boolean GENERATED ALWAYS AS (true) VIRTUAL;
ALTER TABLE analytics.alert_rules
    ADD COLUMN runtime_lock_guard boolean GENERATED ALWAYS AS (true) VIRTUAL;
ALTER TABLE history.work_reservations
    ADD COLUMN runtime_lock_guard boolean GENERATED ALWAYS AS (true) VIRTUAL;

COMMENT ON COLUMN metadata.tenants.runtime_lock_guard IS
    'Immutable column-level UPDATE grant for runtime lifecycle row locks.';
COMMENT ON COLUMN ingestion.replay_sessions.runtime_lock_guard IS
    'Immutable column-level UPDATE grant for ingestion and recovery session row locks.';
COMMENT ON COLUMN analytics.calculator_configurations.runtime_lock_guard IS
    'Immutable column-level UPDATE grant for worker locks on operator configuration.';
COMMENT ON COLUMN analytics.alert_rules.runtime_lock_guard IS
    'Immutable column-level UPDATE grant for worker locks on operator rule selection.';
COMMENT ON COLUMN history.work_reservations.runtime_lock_guard IS
    'Immutable column-level UPDATE grant for operators that lock a reservation without editing it.';
