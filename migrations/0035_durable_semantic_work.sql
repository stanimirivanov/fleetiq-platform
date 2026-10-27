-- Durable semantic processing, policy version 1.
-- One tenant-owned work item refers to one immutable decoder output. A trigger
-- publishes this handoff in the source transaction: rollback cannot orphan work.
-- No source values or raw bytes are copied. Existing output discovery is an
-- incremental, 32-row keyset traversal; this migration does not backfill data.
-- Installation takes short DDL locks on ingestion.decoded_observations while
-- registering the trigger. Run before starting the checked-schema worker.

CREATE TABLE history.semantic_work (
    tenant_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    protocol_package_id TEXT NOT NULL,
    protocol_package_version TEXT NOT NULL,
    output_index BIGINT NOT NULL,
    -- Policy 1 prefers preserved device time, otherwise preserved receipt time.
    policy_version INTEGER NOT NULL DEFAULT 1 CHECK (policy_version = 1),
    event_time_basis TEXT NOT NULL,
    -- Captured once, before any resolution; never refreshed during retry.
    known_at_ms BIGINT NOT NULL,
    generation INTEGER NOT NULL DEFAULT 0,
    lease_until_ms BIGINT,
    status TEXT NOT NULL DEFAULT 'pending',
    -- NULL means no decision has committed. Once selected, both stay immutable.
    binding_id TEXT,
    binding_revision NUMERIC,
    reason TEXT,
    CONSTRAINT pk_semantic_work PRIMARY KEY
        (tenant_id, message_id, protocol_package_id, protocol_package_version, output_index),
    CONSTRAINT fk_semantic_work_observation FOREIGN KEY
        (tenant_id, message_id, protocol_package_id, protocol_package_version, output_index)
        REFERENCES ingestion.decoded_observations
        (tenant_id, message_id, protocol_package_id, protocol_package_version, output_index)
        ON DELETE NO ACTION,
    CONSTRAINT fk_semantic_work_binding FOREIGN KEY (tenant_id, binding_id, binding_revision)
        REFERENCES metadata.signal_binding_revisions (tenant_id, binding_id, revision)
        MATCH SIMPLE ON DELETE NO ACTION,
    CONSTRAINT ck_semantic_work_selection CHECK
        ((binding_id IS NULL) = (binding_revision IS NULL)),
    CONSTRAINT ck_semantic_work_basis CHECK
        (event_time_basis IN ('device_recorded', 'received_at')),
    CONSTRAINT ck_semantic_work_generation CHECK (generation BETWEEN 0 AND 3),
    CONSTRAINT ck_semantic_work_lease CHECK
        ((generation = 0 AND lease_until_ms IS NULL) OR (generation > 0 AND lease_until_ms IS NOT NULL)),
    CONSTRAINT ck_semantic_work_outcome CHECK (
        (status = 'pending' AND reason IS NULL)
        OR (status = 'completed' AND binding_id IS NOT NULL AND reason IS NULL)
        OR (status = 'unresolved' AND binding_id IS NULL AND reason IS NOT NULL AND reason IN
            ('no_binding', 'ambiguous', 'missing_definition', 'value_kind', 'unit'))
        OR (status = 'exhausted' AND reason IS NULL))
);
COMMENT ON TABLE history.semantic_work IS
    'Recoverable policy-v1 interpretation of an exact decoded output; terminal outcomes and immutable selection survive worker restart.';
COMMENT ON COLUMN history.semantic_work.known_at_ms IS
    'Inclusive recorded-time cutoff, in signed Unix milliseconds, captured at first enqueue.';
COMMENT ON COLUMN history.semantic_work.generation IS
    'Monotone attempt fence, maximum three; expiry allows a new owner, never an unfenced completion.';
COMMENT ON COLUMN history.semantic_work.binding_revision IS
    'Exact immutable binding selected before append; retries must not resolve a later revision.';
COMMENT ON COLUMN history.semantic_work.reason IS
    'Closed non-sensitive unresolved category; source reference retains full evidence without duplicating telemetry.';
-- Claim traversal excludes completed work. Expiry ordering is deterministic.
CREATE INDEX ix_semantic_work_pending ON history.semantic_work
    (tenant_id, message_id, protocol_package_id, protocol_package_version, output_index)
    WHERE status = 'pending';

-- Work is mutable scheduling state, but meaning and terminal evidence are not.
CREATE FUNCTION history.guard_semantic_work() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'semantic work evidence cannot be deleted';
    END IF;
    IF ROW(NEW.tenant_id, NEW.message_id, NEW.protocol_package_id,
           NEW.protocol_package_version, NEW.output_index, NEW.policy_version,
           NEW.event_time_basis, NEW.known_at_ms)
       IS DISTINCT FROM ROW(OLD.tenant_id, OLD.message_id, OLD.protocol_package_id,
           OLD.protocol_package_version, OLD.output_index, OLD.policy_version,
           OLD.event_time_basis, OLD.known_at_ms)
       OR (OLD.binding_id IS NOT NULL AND
           ROW(NEW.binding_id, NEW.binding_revision) IS DISTINCT FROM
           ROW(OLD.binding_id, OLD.binding_revision))
       OR NEW.generation < OLD.generation
       OR (OLD.status <> 'pending' AND NEW IS DISTINCT FROM OLD) THEN
        RAISE EXCEPTION 'semantic work meaning or terminal evidence is immutable';
    END IF;
    RETURN NEW;
END;
$$;
COMMENT ON FUNCTION history.guard_semantic_work() IS
    'Protects original interpretation choices and terminal evidence while allowing fenced scheduling updates.';
CREATE TRIGGER guard_semantic_work BEFORE UPDATE OR DELETE ON history.semantic_work
    FOR EACH ROW EXECUTE FUNCTION history.guard_semantic_work();

-- This is the versioned decoded-output -> history handoff, not decoder policy.
CREATE FUNCTION history.enqueue_semantic_observation() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO history.semantic_work
        (tenant_id, message_id, protocol_package_id, protocol_package_version,
         output_index, event_time_basis, known_at_ms)
    VALUES (NEW.tenant_id, NEW.message_id, NEW.protocol_package_id,
        NEW.protocol_package_version, NEW.output_index,
        CASE WHEN NEW.device_recorded_at_ms IS NULL THEN 'received_at' ELSE 'device_recorded' END,
        floor(extract(epoch FROM clock_timestamp()) * 1000)::bigint);
    RETURN NEW;
END;
$$;
COMMENT ON FUNCTION history.enqueue_semantic_observation() IS
    'Atomically queues each new durable decoded output with explicit policy-v1 time choices.';
CREATE TRIGGER enqueue_semantic_observation AFTER INSERT ON ingestion.decoded_observations
    FOR EACH ROW EXECUTE FUNCTION history.enqueue_semantic_observation();

-- The singleton cursor bounds old-source discovery, not ongoing ingestion.
-- New inserts behind the cursor are covered by the transactional handoff above.
CREATE TABLE history.semantic_backfill (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    tenant_id TEXT, message_id TEXT, protocol_package_id TEXT,
    protocol_package_version TEXT, output_index BIGINT,
    completed BOOLEAN NOT NULL DEFAULT FALSE,
    CONSTRAINT ck_semantic_backfill_cursor CHECK (
        (tenant_id IS NULL AND message_id IS NULL AND protocol_package_id IS NULL
         AND protocol_package_version IS NULL AND output_index IS NULL)
        OR (tenant_id IS NOT NULL AND message_id IS NOT NULL AND protocol_package_id IS NOT NULL
         AND protocol_package_version IS NOT NULL AND output_index IS NOT NULL))
);
COMMENT ON TABLE history.semantic_backfill IS
    'Single locked keyset checkpoint for bounded discovery of pre-migration outputs; source keys are opaque ordering, not event chronology.';
INSERT INTO history.semantic_backfill (singleton) VALUES (TRUE);
