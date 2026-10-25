-- Revisioned protocol activation belongs to one registered device and host
-- channel. There is no global identifier discovery or implicit device creation.
-- Only new tables are introduced: existing data is not rewritten or upgraded.
-- A short tenant SHARE lock orders commands with tenant suspension. Head rows
-- serialize competing edits; immutable history preserves actor/time and retry
-- evidence. One SQL transaction advances the head and appends its generation.
CREATE TABLE ingestion.protocol_route_heads (
    tenant_id TEXT NOT NULL,
    device_asset_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    revision BIGINT NOT NULL,
    CONSTRAINT pk_protocol_route_heads PRIMARY KEY (tenant_id,device_asset_id,channel_id),
    CONSTRAINT fk_protocol_route_heads_device FOREIGN KEY (tenant_id,device_asset_id)
        REFERENCES metadata.communication_devices (tenant_id,device_asset_id) ON DELETE RESTRICT,
    CONSTRAINT ck_protocol_route_heads_channel CHECK (channel_id ~ '^[A-Za-z0-9_.-]{1,128}$'),
    CONSTRAINT ck_protocol_route_heads_revision CHECK (revision BETWEEN 0 AND 4294967295)
);
COMMENT ON TABLE ingestion.protocol_route_heads IS 'Serialization point for device/channel configuration, never peer authentication. Revision zero exists only inside the initial command transaction.';
COMMENT ON COLUMN ingestion.protocol_route_heads.channel_id IS 'Trusted deployment/listener namespace; never selected by a guest identifier claim.';
COMMENT ON COLUMN ingestion.protocol_route_heads.revision IS 'Current monotonic unsigned-32-bit generation; zero is the initial transaction sentinel. Exhaustion fails closed.';

CREATE TABLE ingestion.protocol_route_revisions (
    tenant_id TEXT NOT NULL,
    device_asset_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    revision BIGINT NOT NULL,
    protocol_package_id TEXT,
    protocol_package_version TEXT,
    wit_world TEXT,
    policy_version INTEGER,
    actor_id TEXT NOT NULL,
    recorded_at_ms BIGINT NOT NULL,
    CONSTRAINT pk_protocol_route_revisions PRIMARY KEY (tenant_id,device_asset_id,channel_id,revision),
    CONSTRAINT fk_protocol_route_revisions_head FOREIGN KEY (tenant_id,device_asset_id,channel_id)
        REFERENCES ingestion.protocol_route_heads (tenant_id,device_asset_id,channel_id) ON DELETE RESTRICT,
    -- The selection check rejects partially null selections. All-null retires
    -- this route; all-present pins successful evidence for this exact world.
    CONSTRAINT fk_protocol_route_revisions_admission FOREIGN KEY
        (tenant_id,protocol_package_id,protocol_package_version,wit_world,policy_version)
        REFERENCES ingestion.protocol_admissions
        (tenant_id,protocol_package_id,protocol_package_version,wit_world,policy_version) ON DELETE RESTRICT,
    CONSTRAINT ck_protocol_route_revisions_selection CHECK (
        (protocol_package_id IS NULL AND protocol_package_version IS NULL AND wit_world IS NULL AND policy_version IS NULL)
        OR (protocol_package_id IS NOT NULL AND protocol_package_version IS NOT NULL AND wit_world IS NOT NULL AND policy_version IS NOT NULL)),
    CONSTRAINT fk_protocol_route_revisions_actor FOREIGN KEY (actor_id) REFERENCES identity.actors (actor_id) ON DELETE RESTRICT,
    CONSTRAINT ck_protocol_route_revisions_revision CHECK (revision BETWEEN 1 AND 4294967295),
    CONSTRAINT ck_protocol_route_revisions_time CHECK (recorded_at_ms >= 0),
    CONSTRAINT ck_protocol_route_revisions_policy CHECK (policy_version IS NULL OR policy_version > 0)
);
COMMENT ON TABLE ingestion.protocol_route_revisions IS 'Immutable activation/retirement history. Each new generation invalidates prior session handoffs; retirement preserves stored artifacts and admission evidence.';
COMMENT ON COLUMN ingestion.protocol_route_revisions.policy_version IS 'Admission policy pinned when commissioned. Current routing also checks the host current policy; old evidence is not automatically upgraded.';
COMMENT ON COLUMN ingestion.protocol_route_revisions.recorded_at_ms IS 'Original host receipt time in Unix milliseconds; exact delivery retries preserve it.';
COMMENT ON COLUMN ingestion.protocol_route_revisions.actor_id IS 'Authenticated protocol.manage author; same-actor/content/generation retries resolve the first outcome.';
CREATE TRIGGER protocol_route_revisions_immutable BEFORE UPDATE OR DELETE ON ingestion.protocol_route_revisions
    FOR EACH ROW EXECUTE FUNCTION ingestion.reject_protocol_artifact_mutation();
-- The composite primary keys serve exact device/channel reads. Identifier
-- resolution starts from metadata's existing tenant/kind/authority/value unique
-- index and joins this exact head; no whole-fleet or per-row lookup loop exists.

-- Deferred circular integrity permits the initial zero sentinel only inside a
-- transaction: no head can commit without its exact immutable history row.
ALTER TABLE ingestion.protocol_route_heads ADD CONSTRAINT fk_protocol_route_heads_current
    FOREIGN KEY (tenant_id,device_asset_id,channel_id,revision)
    REFERENCES ingestion.protocol_route_revisions (tenant_id,device_asset_id,channel_id,revision)
    DEFERRABLE INITIALLY DEFERRED;
CREATE FUNCTION ingestion.enforce_protocol_route_head() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        IF NEW.revision<>0 THEN RAISE EXCEPTION 'route must start at zero' USING ERRCODE='23514'; END IF;
    ELSIF (NEW.tenant_id,NEW.device_asset_id,NEW.channel_id) IS DISTINCT FROM (OLD.tenant_id,OLD.device_asset_id,OLD.channel_id)
        OR NEW.revision<>OLD.revision+1 THEN
        RAISE EXCEPTION 'route generation must advance exactly once' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$;
COMMENT ON FUNCTION ingestion.enforce_protocol_route_head() IS 'Reject identity changes, skipped generations and rollback to historical routes; deferred FK forbids a committed zero sentinel.';
CREATE TRIGGER protocol_route_head_sequence BEFORE INSERT OR UPDATE ON ingestion.protocol_route_heads
    FOR EACH ROW EXECUTE FUNCTION ingestion.enforce_protocol_route_head();

CREATE FUNCTION ingestion.enforce_protocol_route_append() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.revision IS DISTINCT FROM (SELECT revision FROM ingestion.protocol_route_heads
        WHERE tenant_id=NEW.tenant_id AND device_asset_id=NEW.device_asset_id AND channel_id=NEW.channel_id) THEN
        RAISE EXCEPTION 'history must match current route generation' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$;
COMMENT ON FUNCTION ingestion.enforce_protocol_route_append() IS 'Reject orphan, future or historical insertions; a route command advances the locked head before appending its exact generation.';
CREATE TRIGGER protocol_route_append_sequence BEFORE INSERT ON ingestion.protocol_route_revisions
    FOR EACH ROW EXECUTE FUNCTION ingestion.enforce_protocol_route_append();
