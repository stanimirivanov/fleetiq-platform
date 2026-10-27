-- Durable current-property cache and pending work (policy version 1).
-- History and exact binding/source facts remain authoritative. A queue entry is
-- written in the same transaction as each new interpretation, including a
-- migration backfill. Identity sequence numbers schedule work ONLY; they are
-- never a processed watermark because allocation and commit order can differ.
--
-- Each tenant has one locked checkpoint row. A batch consumes <=100 queue rows,
-- upserts deterministic per-property winners and advances its revision together.
-- Rebuild runs under REPEATABLE READ and the same lock: deletion/recomputation,
-- visible queue completion and generation advancement commit atomically.
-- All objects are additive. The backfill scans history and the trigger takes a
-- schema lock on that table; schedule migration while semantic writers are
-- quiesced for large histories. No history, source or metadata bytes are changed.
CREATE SCHEMA projection;

-- Revisions count committed work batches/rebuilds, not arrivals or timestamps.
-- Generation increments on rebuild; revision never resets (including rebuild).
-- Signed BIGINT storage fails on overflow instead of wrapping continuity tokens.
CREATE TABLE projection.tenant_heads (
    tenant_id TEXT NOT NULL,
    generation BIGINT NOT NULL DEFAULT 1,
    revision BIGINT NOT NULL DEFAULT 0,
    processed_count BIGINT NOT NULL DEFAULT 0,
    last_served_at TIMESTAMPTZ NOT NULL DEFAULT '-infinity',
    CONSTRAINT pk_projection_tenant_heads PRIMARY KEY (tenant_id),
    CONSTRAINT fk_projection_tenant_heads_tenant FOREIGN KEY (tenant_id)
        REFERENCES metadata.tenants (tenant_id) ON DELETE NO ACTION,
    CONSTRAINT ck_projection_tenant_heads_counters CHECK
        (generation > 0 AND revision >= 0 AND processed_count >= 0)
);

-- One exact immutable interpretation reference; no copied value or payload.
CREATE TABLE projection.pending_changes (
    tenant_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    protocol_package_id TEXT NOT NULL,
    protocol_package_version TEXT NOT NULL,
    output_index BIGINT NOT NULL,
    binding_id TEXT NOT NULL,
    binding_revision NUMERIC NOT NULL,
    event_time_basis TEXT NOT NULL,
    queue_id BIGINT GENERATED ALWAYS AS IDENTITY,
    CONSTRAINT pk_projection_pending_changes PRIMARY KEY (tenant_id, message_id, protocol_package_id, protocol_package_version, output_index, binding_id, binding_revision, event_time_basis),
    CONSTRAINT uq_projection_pending_queue UNIQUE (tenant_id, queue_id),
    CONSTRAINT fk_projection_pending_changes_history FOREIGN KEY (tenant_id, message_id, protocol_package_id, protocol_package_version, output_index, binding_id, binding_revision, event_time_basis)
        REFERENCES history.asset_property_changes (tenant_id, message_id, protocol_package_id, protocol_package_version, output_index, binding_id, binding_revision, event_time_basis) ON DELETE NO ACTION
);
-- The unique tenant/queue index supports oldest-work selection and prevents
-- explicitly overridden sequence IDs from enlarging a nominal 100-row batch.

-- Current winner pointer per exact installed property version. Values/quality
-- stay in immutable decoded storage. A trigger validates denormalized target
-- and event time against the referenced history/binding, including tenant.
CREATE TABLE projection.current_properties (
    tenant_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    protocol_package_id TEXT NOT NULL,
    protocol_package_version TEXT NOT NULL,
    output_index BIGINT NOT NULL,
    binding_id TEXT NOT NULL,
    binding_revision NUMERIC NOT NULL,
    event_time_basis TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    property_id TEXT NOT NULL,
    property_version BIGINT NOT NULL,
    event_at_ms BIGINT NOT NULL,
    CONSTRAINT pk_projection_current_properties PRIMARY KEY (tenant_id, asset_id, property_id, property_version),
    CONSTRAINT fk_projection_current_properties_history FOREIGN KEY (tenant_id, message_id, protocol_package_id, protocol_package_version, output_index, binding_id, binding_revision, event_time_basis)
        REFERENCES history.asset_property_changes (tenant_id, message_id, protocol_package_id, protocol_package_version, output_index, binding_id, binding_revision, event_time_basis) ON DELETE NO ACTION
);

-- A foreign key alone cannot assert the binding's target and selected time.
CREATE FUNCTION projection.check_current_lineage() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM history.asset_property_changes h
        JOIN metadata.signal_binding_revisions b
          ON b.tenant_id=h.tenant_id AND b.binding_id=h.binding_id AND b.revision=h.binding_revision
        WHERE h.tenant_id = NEW.tenant_id
          AND h.message_id = NEW.message_id
          AND h.protocol_package_id = NEW.protocol_package_id
          AND h.protocol_package_version = NEW.protocol_package_version
          AND h.output_index = NEW.output_index
          AND h.binding_id = NEW.binding_id
          AND h.binding_revision = NEW.binding_revision
          AND h.event_time_basis = NEW.event_time_basis
          AND h.event_at_ms=NEW.event_at_ms AND b.target_asset_id=NEW.asset_id
          AND b.target_property_id=NEW.property_id AND b.target_property_version=NEW.property_version
    ) THEN
        RAISE EXCEPTION 'Current property lineage is inconsistent'
            USING ERRCODE='23514', CONSTRAINT='ck_projection_current_lineage';
    END IF;
    RETURN NEW;
END; $$;
CREATE TRIGGER current_properties_check_lineage BEFORE INSERT OR UPDATE
ON projection.current_properties FOR EACH ROW EXECUTE FUNCTION projection.check_current_lineage();

-- The AFTER trigger sees only successfully appended facts, so exact retries
-- with ON CONFLICT DO NOTHING create neither work nor new revisions.
CREATE FUNCTION projection.enqueue_property_change() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO projection.tenant_heads (tenant_id) VALUES (NEW.tenant_id) ON CONFLICT DO NOTHING;
    INSERT INTO projection.pending_changes (tenant_id, message_id, protocol_package_id, protocol_package_version, output_index, binding_id, binding_revision, event_time_basis)
    VALUES (NEW.tenant_id, NEW.message_id, NEW.protocol_package_id, NEW.protocol_package_version, NEW.output_index, NEW.binding_id, NEW.binding_revision, NEW.event_time_basis) ON CONFLICT DO NOTHING;
    RETURN NEW;
END; $$;
CREATE TRIGGER asset_property_changes_enqueue_current AFTER INSERT
ON history.asset_property_changes FOR EACH ROW EXECUTE FUNCTION projection.enqueue_property_change();

-- Install the head before queue backfill. Migration/table locking prevents a
-- gap between historical facts and the new transactional enqueue trigger.
INSERT INTO projection.tenant_heads (tenant_id)
SELECT DISTINCT tenant_id FROM history.asset_property_changes ON CONFLICT DO NOTHING;
INSERT INTO projection.pending_changes (tenant_id, message_id, protocol_package_id, protocol_package_version, output_index, binding_id, binding_revision, event_time_basis)
SELECT tenant_id, message_id, protocol_package_id, protocol_package_version, output_index, binding_id, binding_revision, event_time_basis FROM history.asset_property_changes ON CONFLICT DO NOTHING;

-- Serializes a single tenant, while another worker can progress another tenant.
-- Locks active lifecycle and the head for the complete transaction. The
-- function has invoker privileges; it is not a security-definer authorization bypass.
CREATE FUNCTION projection.project_next()
RETURNS TABLE (applied_count BIGINT)
LANGUAGE plpgsql AS $$
DECLARE selected_tenant TEXT; consumed BIGINT;
BEGIN
    SELECT t.tenant_id INTO selected_tenant
    FROM metadata.tenants t JOIN projection.tenant_heads h USING (tenant_id)
    WHERE t.is_active AND EXISTS (SELECT 1 FROM projection.pending_changes q WHERE q.tenant_id=t.tenant_id)
    ORDER BY h.last_served_at, t.tenant_id COLLATE "C"
    FOR SHARE OF t SKIP LOCKED FOR UPDATE OF h SKIP LOCKED LIMIT 1;
    IF selected_tenant IS NULL THEN RETURN; END IF;
    WITH selected AS MATERIALIZED (
        SELECT q.queue_id FROM projection.pending_changes q WHERE q.tenant_id=selected_tenant
        ORDER BY q.queue_id LIMIT 100
    ), removed AS (
        DELETE FROM projection.pending_changes q USING selected s
        WHERE q.tenant_id=selected_tenant AND q.queue_id=s.queue_id RETURNING q.*
    ), candidates AS (
        SELECT DISTINCT ON (b.target_asset_id,b.target_property_id,b.target_property_version)
            h.tenant_id, h.message_id, h.protocol_package_id,
            h.protocol_package_version, h.output_index,
            h.binding_id, h.binding_revision,
            h.event_time_basis, b.target_asset_id AS asset_id,
            b.target_property_id AS property_id,b.target_property_version AS property_version,h.event_at_ms
        FROM removed r JOIN history.asset_property_changes h ON h.tenant_id = r.tenant_id
          AND h.message_id = r.message_id
          AND h.protocol_package_id = r.protocol_package_id
          AND h.protocol_package_version = r.protocol_package_version
          AND h.output_index = r.output_index
          AND h.binding_id = r.binding_id
          AND h.binding_revision = r.binding_revision
          AND h.event_time_basis = r.event_time_basis
        JOIN metadata.signal_binding_revisions b ON b.tenant_id=h.tenant_id
          AND b.binding_id=h.binding_id AND b.revision=h.binding_revision
        ORDER BY b.target_asset_id,b.target_property_id,b.target_property_version,
            h.event_at_ms DESC, h.message_id COLLATE "C" DESC, h.protocol_package_id COLLATE "C" DESC,
            h.protocol_package_version COLLATE "C" DESC, h.output_index DESC,
            h.binding_id COLLATE "C" DESC, h.binding_revision DESC,
            h.event_time_basis COLLATE "C" DESC
    ), changed AS (
        INSERT INTO projection.current_properties AS incumbent (tenant_id, message_id, protocol_package_id, protocol_package_version, output_index, binding_id, binding_revision, event_time_basis,asset_id,property_id,property_version,event_at_ms)
        SELECT * FROM candidates
        ON CONFLICT (tenant_id,asset_id,property_id,property_version) DO UPDATE SET
            message_id=EXCLUDED.message_id, protocol_package_id=EXCLUDED.protocol_package_id,
            protocol_package_version=EXCLUDED.protocol_package_version, output_index=EXCLUDED.output_index,
            binding_id=EXCLUDED.binding_id, binding_revision=EXCLUDED.binding_revision,
            event_time_basis=EXCLUDED.event_time_basis, event_at_ms=EXCLUDED.event_at_ms
        WHERE (EXCLUDED.event_at_ms, EXCLUDED.message_id COLLATE "C",
               EXCLUDED.protocol_package_id COLLATE "C", EXCLUDED.protocol_package_version COLLATE "C",
               EXCLUDED.output_index, EXCLUDED.binding_id COLLATE "C",
               EXCLUDED.binding_revision, EXCLUDED.event_time_basis COLLATE "C")
            > (incumbent.event_at_ms, incumbent.message_id COLLATE "C",
               incumbent.protocol_package_id COLLATE "C", incumbent.protocol_package_version COLLATE "C",
               incumbent.output_index, incumbent.binding_id COLLATE "C",
               incumbent.binding_revision, incumbent.event_time_basis COLLATE "C")
        RETURNING 1
    ) SELECT count(*) INTO consumed FROM removed;
    IF consumed > 0 THEN
        UPDATE projection.tenant_heads SET revision=revision+1, processed_count=processed_count+consumed,
            last_served_at=clock_timestamp()
        WHERE tenant_id=selected_tenant;
        RETURN QUERY SELECT consumed;
    END IF;
END; $$;

-- Caller MUST open REPEATABLE READ before calling. This guards an unsafe
-- standalone invocation. Commits outside this snapshot keep their queue rows.
-- A serializable conflict or timeout aborts the entire rebuild; retry unchanged.
CREATE FUNCTION projection.rebuild_current(selected_tenant TEXT) RETURNS VOID
LANGUAGE plpgsql AS $$
BEGIN
    IF current_setting('transaction_isolation') NOT IN ('repeatable read','serializable') THEN
        RAISE EXCEPTION 'Rebuild requires a consistent transaction' USING ERRCODE='25001';
    END IF;
    PERFORM 1 FROM metadata.tenants WHERE tenant_id=selected_tenant AND is_active FOR SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Active tenant required' USING ERRCODE='23514'; END IF;
    INSERT INTO projection.tenant_heads (tenant_id) VALUES (selected_tenant) ON CONFLICT DO NOTHING;
    PERFORM 1 FROM projection.tenant_heads WHERE tenant_id=selected_tenant FOR UPDATE;
    DELETE FROM projection.current_properties WHERE tenant_id=selected_tenant;
    INSERT INTO projection.current_properties (tenant_id, message_id, protocol_package_id, protocol_package_version, output_index, binding_id, binding_revision, event_time_basis,asset_id,property_id,property_version,event_at_ms)
    SELECT DISTINCT ON (b.target_asset_id,b.target_property_id,b.target_property_version)
        h.tenant_id, h.message_id, h.protocol_package_id,
            h.protocol_package_version, h.output_index,
            h.binding_id, h.binding_revision,
            h.event_time_basis,b.target_asset_id,b.target_property_id,b.target_property_version,h.event_at_ms
    FROM history.asset_property_changes h JOIN metadata.signal_binding_revisions b
      ON b.tenant_id=h.tenant_id AND b.binding_id=h.binding_id AND b.revision=h.binding_revision
    WHERE h.tenant_id=selected_tenant
    ORDER BY b.target_asset_id,b.target_property_id,b.target_property_version,
        h.event_at_ms DESC, h.message_id COLLATE "C" DESC, h.protocol_package_id COLLATE "C" DESC,
            h.protocol_package_version COLLATE "C" DESC, h.output_index DESC,
            h.binding_id COLLATE "C" DESC, h.binding_revision DESC,
            h.event_time_basis COLLATE "C" DESC;
    DELETE FROM projection.pending_changes WHERE tenant_id=selected_tenant;
    UPDATE projection.tenant_heads SET generation=generation+1,revision=revision+1,
        processed_count=(SELECT count(*) FROM history.asset_property_changes WHERE tenant_id=selected_tenant)
    WHERE tenant_id=selected_tenant;
END; $$;

COMMENT ON SCHEMA projection IS 'Rebuildable current-property read models; history is authoritative.';
COMMENT ON TABLE projection.tenant_heads IS 'Per-tenant serialized revision, rebuild generation and processed-count checkpoint.';
COMMENT ON COLUMN projection.tenant_heads.generation IS 'Increments atomically on rebuild; consumers must resnapshot after a generation change.';
COMMENT ON COLUMN projection.tenant_heads.last_served_at IS 'Least-recently-served tenant scheduling hint only; never value precedence or a processed watermark.';
COMMENT ON COLUMN projection.tenant_heads.revision IS 'Advances once per committed nonempty batch or rebuild; not an event-time watermark.';
COMMENT ON COLUMN projection.tenant_heads.processed_count IS 'Consumed interpretation count; reset to snapshot-visible history count on rebuild.';
COMMENT ON TABLE projection.pending_changes IS 'Transactionally enqueued exact interpretations; removed only with committed projection/checkpoint updates.';
COMMENT ON COLUMN projection.pending_changes.queue_id IS 'Scheduling hint only; never a safe committed watermark or provenance identifier.';
COMMENT ON TABLE projection.current_properties IS 'Winner pointers by exact asset/property/version; values, quality and provenance remain immutable history/source facts.';
COMMENT ON COLUMN projection.current_properties.property_version IS 'Exact installed property definition version, checked against immutable binding lineage; never mixed with another version.';
COMMENT ON COLUMN projection.current_properties.event_at_ms IS 'Signed Unix-millisecond selected event time; primary precedence, validated against history.';
COMMENT ON FUNCTION projection.check_current_lineage() IS 'Rejects denormalized target/time disagreement with the exact immutable interpretation.';
COMMENT ON FUNCTION projection.enqueue_property_change() IS 'Atomic history-to-projection pending work; exact duplicate appends create no new work.';
COMMENT ON FUNCTION projection.project_next() IS 'At most 100 queued facts per active tenant; atomically publishes winners and checkpoint with invoker privileges.';
COMMENT ON FUNCTION projection.rebuild_current(TEXT) IS 'Consistent atomic tenant replay; preserves queue entries outside the caller repeatable-read snapshot.';
