-- Additive archive-only state. No history rewrite, deletion or worker activation.
-- Jobs freeze exact identities before object I/O; only verified terminal rows are manifests.
CREATE TABLE history.archive_jobs (
    tenant_id TEXT NOT NULL REFERENCES metadata.tenants(tenant_id),
    job_id BIGINT GENERATED ALWAYS AS IDENTITY,
    generation BIGINT NOT NULL DEFAULT 0 CHECK (generation >= 0),
    lease_until TIMESTAMPTZ,
    reservation_id BIGINT,
    -- Whole physical object checksum/length, never an event-time completeness watermark.
    object_sha256 BYTEA,
    object_bytes BIGINT,
    CONSTRAINT pk_archive_jobs PRIMARY KEY (tenant_id,job_id),
    CONSTRAINT ck_archive_jobs_object CHECK (
        (object_sha256 IS NULL AND object_bytes IS NULL) OR
        (object_sha256 IS NOT NULL AND object_bytes IS NOT NULL AND octet_length(object_sha256)=32 AND object_bytes BETWEEN 1 AND 2097152)),
    CONSTRAINT ck_archive_jobs_claim CHECK (
        (generation=0 AND lease_until IS NULL AND reservation_id IS NULL) OR
        (generation>0 AND lease_until IS NOT NULL AND reservation_id IS NOT NULL))
);
COMMENT ON TABLE history.archive_jobs IS 'Frozen bounded archive selections; non-null complete-object evidence denotes verified manifest publication. Upload alone does not publish coverage.';
CREATE INDEX ix_archive_jobs_pending ON history.archive_jobs(tenant_id,job_id) WHERE object_sha256 IS NULL;

-- Membership preserves the full interpretation identity and tenant FK path.
CREATE TABLE history.archive_members (
    tenant_id TEXT NOT NULL,
    job_id BIGINT NOT NULL,
    message_id TEXT NOT NULL,
    protocol_package_id TEXT NOT NULL,
    protocol_package_version TEXT NOT NULL,
    output_index BIGINT NOT NULL,
    binding_id TEXT NOT NULL,
    binding_revision NUMERIC NOT NULL,
    event_time_basis TEXT NOT NULL,
    CONSTRAINT pk_archive_members PRIMARY KEY (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis),
    CONSTRAINT fk_archive_members_job FOREIGN KEY (tenant_id,job_id) REFERENCES history.archive_jobs(tenant_id,job_id) ON DELETE NO ACTION,
    CONSTRAINT fk_archive_members_change FOREIGN KEY (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis)
        REFERENCES history.asset_property_changes(tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis) ON DELETE NO ACTION
);
CREATE INDEX ix_archive_members_job ON history.archive_members(tenant_id,job_id);
COMMENT ON TABLE history.archive_members IS 'Immutable exact membership, unique across jobs; late arrivals remain unassigned until a subsequent selection.';

CREATE FUNCTION history.guard_archive_evidence() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_TABLE_NAME='archive_members' THEN
        IF TG_OP <> 'INSERT' THEN RAISE EXCEPTION 'archive membership is immutable'; END IF;
        PERFORM 1 FROM history.archive_jobs WHERE tenant_id=NEW.tenant_id AND job_id=NEW.job_id AND generation=0 FOR UPDATE;
        IF NOT FOUND THEN RAISE EXCEPTION 'archive selection already frozen'; END IF;
        IF (SELECT count(*) FROM history.archive_members WHERE tenant_id=NEW.tenant_id AND job_id=NEW.job_id)>=16 THEN
            RAISE EXCEPTION 'archive selection exceeds bound';
        END IF;
    ELSE
        IF NEW.object_sha256 IS NOT NULL AND
           (NEW.generation=0 OR NOT EXISTS (SELECT 1 FROM history.archive_members WHERE tenant_id=NEW.tenant_id AND job_id=NEW.job_id)) THEN
            RAISE EXCEPTION 'empty or unclaimed archive cannot publish';
        END IF;
        IF TG_OP='DELETE' THEN RAISE EXCEPTION 'archive evidence is retained'; END IF;
        IF OLD.object_sha256 IS NOT NULL OR (NEW.tenant_id,NEW.job_id) IS DISTINCT FROM (OLD.tenant_id,OLD.job_id)
           OR NEW.generation<OLD.generation OR NEW.generation>OLD.generation+1 THEN
            RAISE EXCEPTION 'archive manifest or identity is immutable';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER archive_members_guard BEFORE INSERT OR UPDATE OR DELETE ON history.archive_members FOR EACH ROW EXECUTE FUNCTION history.guard_archive_evidence();
CREATE TRIGGER archive_jobs_guard BEFORE UPDATE OR DELETE ON history.archive_jobs FOR EACH ROW EXECUTE FUNCTION history.guard_archive_evidence();
COMMENT ON FUNCTION history.guard_archive_evidence() IS 'Protects frozen bounded membership and terminal manifest evidence; application completion additionally fences current lease, policy and tenant.';
