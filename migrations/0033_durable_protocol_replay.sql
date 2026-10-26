-- Durable protocol replay: accepted bytes, explicit pre-state and immutable
-- execution identity survive process/socket loss. Sessions serialize turns;
-- mutable leases are scheduling state, not authority or transport delivery.
-- Additive empty tables only: no rewrite/backfill or changes to published facts.

-- A checkpoint pins one registered device and exact admitted component/world.
-- Only completion may advance sequence/state; sequence zero is the empty start.
CREATE TABLE ingestion.replay_sessions (
    tenant_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    device_asset_id TEXT NOT NULL,
    protocol_package_id TEXT NOT NULL,
    protocol_package_version TEXT NOT NULL,
    artifact_sha256 BYTEA NOT NULL,
    wit_world TEXT NOT NULL,
    admission_policy INTEGER NOT NULL,
    execution_policy INTEGER NOT NULL,
    execution_sha256 BYTEA NOT NULL,
    CONSTRAINT ck_replay_sessions_execution_sha256 CHECK(octet_length(execution_sha256)=32),
    current_sequence BIGINT NOT NULL DEFAULT 0,
    state BYTEA NOT NULL DEFAULT ''::bytea,
    CONSTRAINT ck_replay_sessions_admission_policy CHECK(admission_policy=1),
    CONSTRAINT ck_replay_sessions_execution_policy CHECK(execution_policy=1),
    CONSTRAINT pk_replay_sessions PRIMARY KEY (tenant_id,session_id),
    CONSTRAINT ck_replay_sessions_key CHECK (session_id ~ '^[A-Za-z0-9_.-]{1,128}$'),
    CONSTRAINT ck_replay_sessions_sequence CHECK (current_sequence BETWEEN 0 AND 4294967295),
    CONSTRAINT ck_replay_sessions_state CHECK (octet_length(state)<=16384 AND (current_sequence>0 OR octet_length(state)=0)),
    CONSTRAINT fk_replay_sessions_device FOREIGN KEY(tenant_id,device_asset_id) REFERENCES metadata.communication_devices(tenant_id,device_asset_id) ON DELETE RESTRICT,
    CONSTRAINT fk_replay_sessions_content FOREIGN KEY(tenant_id,protocol_package_id,protocol_package_version,artifact_sha256) REFERENCES metadata.protocol_package_releases(tenant_id,protocol_package_id,protocol_package_version,artifact_sha256) ON DELETE RESTRICT,
    CONSTRAINT fk_replay_sessions_admission FOREIGN KEY(tenant_id,protocol_package_id,protocol_package_version,wit_world,admission_policy) REFERENCES ingestion.protocol_admissions(tenant_id,protocol_package_id,protocol_package_version,wit_world,policy_version) ON DELETE RESTRICT
);
COMMENT ON TABLE ingestion.replay_sessions IS 'Tenant-owned serial replay checkpoint; package/world/policy are immutable for the session. No connection or reply-delivery authority.';
COMMENT ON COLUMN ingestion.replay_sessions.execution_sha256 IS 'SHA-256 of reviewed replay policy tag and exact Cargo.lock; incompatible workers leave this session untouched.';
COMMENT ON COLUMN ingestion.replay_sessions.state IS 'Opaque state after current_sequence, at most 16 KiB; sensitive protocol bytes, never routine diagnostics.';
COMMENT ON COLUMN ingestion.replay_sessions.execution_policy IS 'Version 1 fixes frame/state/output/fuel/memory limits; changing interpretation requires a new supported policy, never latest configuration.';

-- One immutable pre-frame snapshot per accepted message. The retained next_state
-- makes a successful completion retry comparable after later turns advance.
CREATE TABLE ingestion.replay_work (
    tenant_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    sequence BIGINT NOT NULL,
    pre_state BYTEA NOT NULL,
    generation BIGINT NOT NULL DEFAULT 0,
    lease_until_ms BIGINT,
    status TEXT NOT NULL DEFAULT 'pending',
    next_state BYTEA,
    CONSTRAINT pk_replay_work PRIMARY KEY(tenant_id,message_id),
    CONSTRAINT uq_replay_work_sequence UNIQUE(tenant_id,session_id,sequence),
    CONSTRAINT fk_replay_work_session FOREIGN KEY(tenant_id,session_id) REFERENCES ingestion.replay_sessions(tenant_id,session_id) ON DELETE RESTRICT,
    CONSTRAINT fk_replay_work_message FOREIGN KEY(tenant_id,message_id) REFERENCES ingestion.accepted_messages(tenant_id,message_id) ON DELETE RESTRICT,
    CONSTRAINT ck_replay_work_sequence CHECK(sequence BETWEEN 1 AND 4294967295),
    CONSTRAINT ck_replay_work_pre_state CHECK(octet_length(pre_state)<=16384),
    CONSTRAINT ck_replay_work_generation CHECK(generation BETWEEN 0 AND 3),
    CONSTRAINT ck_replay_work_status CHECK(status IN ('pending','completed','poisoned')),
    CONSTRAINT ck_replay_work_next_state CHECK((status='completed')=(next_state IS NOT NULL) AND (next_state IS NULL OR octet_length(next_state)<=16384)),
    CONSTRAINT ck_replay_work_lease CHECK((generation=0 AND lease_until_ms IS NULL) OR (generation>0 AND lease_until_ms IS NOT NULL))
);
COMMENT ON TABLE ingestion.replay_work IS 'Immutable replay input with fenced 30-second claims, three attempts, terminal poison/completion and no retained transport replies.';
COMMENT ON COLUMN ingestion.replay_work.pre_state IS 'Exact bytes before this frame, copied from the committed predecessor; never reconstruct from the current checkpoint during replay.';
COMMENT ON COLUMN ingestion.replay_work.generation IS 'Database-owned fencing/attempt number; stale or expired claimants cannot append results or advance a checkpoint.';
COMMENT ON COLUMN ingestion.replay_work.lease_until_ms IS 'Database clock Unix milliseconds; worker clocks never decide lease ownership.';
COMMENT ON COLUMN ingestion.replay_work.next_state IS 'Immutable completion receipt retained for exact retries even after later checkpoints.';
-- A small partial index serves claim/expiry polling, excluding terminal history.
CREATE INDEX ix_replay_work_pending ON ingestion.replay_work(lease_until_ms,tenant_id,message_id) WHERE status='pending';
COMMENT ON INDEX ingestion.ix_replay_work_pending IS 'Bounded pending claim/expiry access; terminal evidence is not reprocessed.';

-- Reject cross-device/package references and skipped/guessed predecessor state
-- even when a writer bypasses the application adapter.
CREATE FUNCTION ingestion.guard_replay_work() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Replay evidence cannot be deleted' USING ERRCODE='55000'; END IF;
    IF TG_OP='INSERT' THEN
        IF NEW.generation<>0 OR NEW.lease_until_ms IS NOT NULL OR NEW.status<>'pending' OR NEW.next_state IS NOT NULL THEN RAISE EXCEPTION 'Replay input must start pending and unclaimed' USING ERRCODE='23514'; END IF;
        IF NOT EXISTS (SELECT 1 FROM ingestion.replay_sessions s JOIN ingestion.accepted_messages m ON m.tenant_id=s.tenant_id AND m.message_id=NEW.message_id
            WHERE s.tenant_id=NEW.tenant_id AND s.session_id=NEW.session_id AND s.current_sequence+1=NEW.sequence AND s.state=NEW.pre_state
            AND m.device_asset_id=s.device_asset_id AND m.protocol_package_id=s.protocol_package_id AND m.protocol_package_version=s.protocol_package_version AND octet_length(m.payload)<=65536)
        THEN RAISE EXCEPTION 'Invalid replay predecessor or provenance' USING ERRCODE='23514'; END IF;
    ELSE
        IF OLD.status<>'pending' OR ROW(NEW.tenant_id,NEW.message_id,NEW.session_id,NEW.sequence,NEW.pre_state) IS DISTINCT FROM ROW(OLD.tenant_id,OLD.message_id,OLD.session_id,OLD.sequence,OLD.pre_state)
        THEN RAISE EXCEPTION 'Replay input and terminal receipts are immutable' USING ERRCODE='55000'; END IF;
    END IF;
    RETURN NEW;
END; $$;
COMMENT ON FUNCTION ingestion.guard_replay_work() IS 'Checks predecessor/message provenance and protects immutable replay input/terminal receipts.';
CREATE TRIGGER replay_work_guard BEFORE INSERT OR UPDATE OR DELETE ON ingestion.replay_work FOR EACH ROW EXECUTE FUNCTION ingestion.guard_replay_work();

-- Checkpoint writes must be paired with one completed work item and its exact
-- decoded marker. Completion and this update occur in the same transaction.
CREATE FUNCTION ingestion.guard_replay_checkpoint() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Replay checkpoints cannot be deleted' USING ERRCODE='55000'; END IF;
    IF TG_OP='INSERT' THEN
        IF NEW.current_sequence<>0 OR octet_length(NEW.state)<>0 THEN RAISE EXCEPTION 'Replay starts at the empty checkpoint' USING ERRCODE='23514'; END IF;
        RETURN NEW;
    END IF;
    IF ROW(NEW.tenant_id,NEW.session_id,NEW.device_asset_id,NEW.protocol_package_id,NEW.protocol_package_version,NEW.artifact_sha256,NEW.wit_world,NEW.admission_policy,NEW.execution_policy,NEW.execution_sha256)
       IS DISTINCT FROM ROW(OLD.tenant_id,OLD.session_id,OLD.device_asset_id,OLD.protocol_package_id,OLD.protocol_package_version,OLD.artifact_sha256,OLD.wit_world,OLD.admission_policy,OLD.execution_policy,OLD.execution_sha256)
       OR NEW.current_sequence<>OLD.current_sequence+1 OR NOT EXISTS (
       SELECT 1 FROM ingestion.replay_work w JOIN ingestion.decoded_message_results d ON d.tenant_id=w.tenant_id AND d.message_id=w.message_id
       WHERE w.tenant_id=NEW.tenant_id AND w.session_id=NEW.session_id AND w.sequence=NEW.current_sequence AND w.status='completed' AND w.next_state=NEW.state
       AND d.protocol_package_id=NEW.protocol_package_id AND d.protocol_package_version=NEW.protocol_package_version)
    THEN RAISE EXCEPTION 'Invalid replay checkpoint transition' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END; $$;
COMMENT ON FUNCTION ingestion.guard_replay_checkpoint() IS 'Permits only a single completed turn with exact decoded-result evidence to advance immutable session identity.';
CREATE TRIGGER replay_checkpoint_guard BEFORE INSERT OR UPDATE OR DELETE ON ingestion.replay_sessions FOR EACH ROW EXECUTE FUNCTION ingestion.guard_replay_checkpoint();

-- Deferred consistency allows the transaction to write completion then head,
-- while rejecting direct half-completions at COMMIT rather than trusting callers.
CREATE FUNCTION ingestion.check_replay_completion() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.status='completed' AND NOT EXISTS (
        SELECT 1 FROM ingestion.replay_sessions s JOIN ingestion.decoded_message_results d ON d.tenant_id=s.tenant_id AND d.message_id=NEW.message_id
        WHERE s.tenant_id=NEW.tenant_id AND s.session_id=NEW.session_id AND s.current_sequence>=NEW.sequence
        AND (s.current_sequence>NEW.sequence OR s.state=NEW.next_state)
        AND d.protocol_package_id=s.protocol_package_id AND d.protocol_package_version=s.protocol_package_version)
    THEN RAISE EXCEPTION 'Incomplete replay completion transaction' USING ERRCODE='23514'; END IF;
    RETURN NULL;
END; $$;
COMMENT ON FUNCTION ingestion.check_replay_completion() IS 'Deferred proof that every completed turn has durable decoded output and a matching advanced checkpoint.';
CREATE CONSTRAINT TRIGGER replay_completion_check AFTER INSERT OR UPDATE ON ingestion.replay_work DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ingestion.check_replay_completion();
