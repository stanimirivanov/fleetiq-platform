-- Tenant-owned immutable fixed-window schedules and durable generation-fenced work.
-- Trusted installation is separate from future HTTP activation. All *_ms values
-- are signed Unix milliseconds, matching calculator contracts; arithmetic uses
-- numeric for overflow checks. Referenced evidence is retained: no cascading deletes.
-- Schedule content never changes; only the aligned cursor advances.

CREATE TABLE analytics.calculator_schedules (
    tenant_id TEXT NOT NULL,
    schedule_id TEXT NOT NULL CHECK(metadata.is_valid_identifier(schedule_id)),
    calculator_id TEXT NOT NULL,
    calculator_version TEXT NOT NULL,
    configuration_id TEXT NOT NULL CHECK(metadata.is_valid_identifier(configuration_id)),
    scope_asset_id TEXT NOT NULL,
    target_asset_id TEXT NOT NULL,
    first_start_ms BIGINT NOT NULL,
    window_ms BIGINT NOT NULL CHECK(window_ms BETWEEN 1 AND 604800000),
    lateness_ms BIGINT NOT NULL CHECK(lateness_ms BETWEEN 0 AND 604800000),
    minimum_samples INTEGER NOT NULL CHECK(minimum_samples BETWEEN 1 AND 1024),
    next_start_ms BIGINT NOT NULL,
    PRIMARY KEY(tenant_id,schedule_id),
    FOREIGN KEY(tenant_id,calculator_id,calculator_version) REFERENCES analytics.calculator_definitions(tenant_id,calculator_id,calculator_version),
    FOREIGN KEY(tenant_id,scope_asset_id) REFERENCES metadata.assets(tenant_id,asset_id),
    FOREIGN KEY(tenant_id,target_asset_id) REFERENCES metadata.assets(tenant_id,asset_id),
    CHECK(first_start_ms::numeric+window_ms+lateness_ms<=9223372036854775807),
    CHECK(next_start_ms>=first_start_ms AND (next_start_ms::numeric-first_start_ms)%window_ms=0)
);
-- Prevent definition replacement or cursor rewind after work has been enumerated.
CREATE FUNCTION analytics.protect_schedule() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' OR (to_jsonb(NEW)-'next_start_ms') IS DISTINCT FROM (to_jsonb(OLD)-'next_start_ms') OR NEW.next_start_ms<OLD.next_start_ms THEN
        RAISE EXCEPTION 'Calculator schedule definition is immutable';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER calculator_schedules_immutable BEFORE UPDATE OR DELETE ON analytics.calculator_schedules FOR EACH ROW EXECUTE FUNCTION analytics.protect_schedule();
-- Original request and first processing time survive every claim generation.
CREATE TABLE analytics.calculator_work (
    tenant_id TEXT NOT NULL,
    evaluation_id TEXT NOT NULL,
    schedule_id TEXT NOT NULL,
    start_ms BIGINT NOT NULL,
    request JSONB NOT NULL,
    generation INTEGER NOT NULL DEFAULT 0 CHECK(generation BETWEEN 0 AND 3),
    lease_until_ms BIGINT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','done','exhausted')),
    PRIMARY KEY(tenant_id,evaluation_id),
    UNIQUE(tenant_id,schedule_id,start_ms),
    FOREIGN KEY(tenant_id,schedule_id) REFERENCES analytics.calculator_schedules(tenant_id,schedule_id),
    CHECK(status<>'done' OR generation>0)
);
-- Pending/expired lookup also supports bounded exhaustion sweeps.
CREATE INDEX ix_calculator_work_claim ON analytics.calculator_work(status,lease_until_ms,tenant_id,evaluation_id);
-- Workers may advance fences and state, never rewrite logical request identity.
CREATE FUNCTION analytics.protect_calculator_work() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' OR (to_jsonb(NEW)-ARRAY['generation','lease_until_ms','status']) IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['generation','lease_until_ms','status'])
       OR NEW.generation<OLD.generation OR OLD.status<>'pending' THEN RAISE EXCEPTION 'Calculator work identity is immutable'; END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER calculator_work_identity BEFORE UPDATE OR DELETE ON analytics.calculator_work FOR EACH ROW EXECUTE FUNCTION analytics.protect_calculator_work();
COMMENT ON TABLE analytics.calculator_work IS
    'Durable logical windows with immutable original request, three generation-fenced 30-second attempts, retained exhaustion and atomic terminal completion. No operator reset API.';

COMMENT ON TABLE analytics.calculator_schedules IS 'Immutable trusted fixed-window definitions with an aligned monotone catch-up cursor; tenant-owned references and retained evidence.';
COMMENT ON COLUMN analytics.calculator_schedules.next_start_ms IS 'First window not yet durably enumerated; Unix milliseconds, advanced atomically with work insertion.';
COMMENT ON COLUMN analytics.calculator_work.lease_until_ms IS 'Database-clock expiry in Unix milliseconds; NULL before first claim. Completion rechecks the lease before commit.';
COMMENT ON COLUMN analytics.calculator_work.request IS 'Immutable original request including logical known-at and first processing time; retries reuse committed preparation.';
COMMENT ON FUNCTION analytics.protect_schedule() IS 'Retains schedule content and prevents backward catch-up cursors.';
COMMENT ON FUNCTION analytics.protect_calculator_work() IS 'Retains request identity, monotone generation and terminal work state.';
