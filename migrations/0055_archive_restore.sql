-- Additive temporary restore state; original archive/history remain immutable and untouched.
-- One slot per tenant/manifest prevents duplicate stored copies from exact request retries.
CREATE TABLE history.restore_jobs (
    tenant_id TEXT NOT NULL,
    manifest_id BIGINT NOT NULL,
    request_generation BIGINT NOT NULL CHECK (request_generation>0),
    attempt BIGINT NOT NULL DEFAULT 0 CHECK (attempt>=0),
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','ready','cancelled','expired')),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (clock_timestamp()+interval '1 hour'),
    lease_until TIMESTAMPTZ,
    reservation_id BIGINT,
    -- Exact verified Parquet file; bounded decoder reconstructs independent original evidence.
    payload BYTEA,
    CONSTRAINT pk_restore_jobs PRIMARY KEY (tenant_id,manifest_id),
    CONSTRAINT fk_restore_jobs_manifest FOREIGN KEY (tenant_id,manifest_id) REFERENCES history.archive_jobs(tenant_id,job_id) ON DELETE NO ACTION,
    CONSTRAINT ck_restore_jobs_ready CHECK ((status='ready')=(payload IS NOT NULL)),
    CONSTRAINT ck_restore_jobs_payload CHECK (payload IS NULL OR octet_length(payload) BETWEEN 1 AND 2097152),
    CONSTRAINT ck_restore_jobs_lease CHECK ((lease_until IS NULL)=(reservation_id IS NULL))
);
CREATE INDEX ix_restore_jobs_expiry ON history.restore_jobs(tenant_id,expires_at,manifest_id) WHERE status IN ('ready','pending');
COMMENT ON TABLE history.restore_jobs IS 'Generation-bound temporary query staging, not replay or authoritative hot history. Expiry/cancellation remove only the derived copy; request identity remains for safe retries.';
COMMENT ON COLUMN history.restore_jobs.payload IS 'Exact whole verified archive file, at most 2 MiB and sixteen interpretations; never re-encoded or re-resolved.';
COMMENT ON COLUMN history.restore_jobs.expires_at IS 'One-hour request lifetime; shortening revokes availability. Same-generation retries cannot extend it.';

CREATE FUNCTION history.guard_restore_staging() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE digest BYTEA; expected_bytes BIGINT;
BEGIN
    IF TG_OP='DELETE' THEN RAISE EXCEPTION 'restore request identities are retained'; END IF;
    SELECT object_sha256,object_bytes INTO digest,expected_bytes FROM history.archive_jobs
      WHERE tenant_id=NEW.tenant_id AND job_id=NEW.manifest_id AND object_sha256 IS NOT NULL;
    IF NOT FOUND THEN RAISE EXCEPTION 'restore requires a published manifest'; END IF;
    IF NEW.expires_at>clock_timestamp()+interval '1 hour' THEN RAISE EXCEPTION 'restore lifetime exceeds bound'; END IF;
    IF TG_OP='INSERT' THEN
        IF NEW.request_generation<>1 OR NEW.attempt<>0 OR NEW.status<>'pending' THEN RAISE EXCEPTION 'invalid initial restore'; END IF;
    ELSE
        IF (NEW.tenant_id,NEW.manifest_id) IS DISTINCT FROM (OLD.tenant_id,OLD.manifest_id)
           OR NEW.request_generation<OLD.request_generation OR NEW.request_generation>OLD.request_generation+1
           OR NEW.attempt<OLD.attempt OR NEW.attempt>OLD.attempt+1 THEN RAISE EXCEPTION 'invalid restore identity/fence'; END IF;
        IF NEW.request_generation=OLD.request_generation THEN
            IF NEW.expires_at>OLD.expires_at OR
               (OLD.status IN ('cancelled','expired') AND NEW.status<>OLD.status) OR
               (OLD.status='ready' AND NEW.status='pending') THEN RAISE EXCEPTION 'restore generation is terminal'; END IF;
        ELSIF NEW.status<>'pending' OR NEW.payload IS NOT NULL OR NEW.lease_until IS NOT NULL OR NEW.reservation_id IS NOT NULL
              OR (OLD.status NOT IN ('cancelled','expired') AND OLD.expires_at>clock_timestamp()) THEN
            RAISE EXCEPTION 'new restore requires terminal prior request';
        END IF;
    END IF;
    IF NEW.payload IS NOT NULL AND (octet_length(NEW.payload)<>expected_bytes OR sha256(NEW.payload)<>digest) THEN
        RAISE EXCEPTION 'restore bytes differ from verified manifest';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER restore_staging_guard BEFORE INSERT OR UPDATE OR DELETE ON history.restore_jobs FOR EACH ROW EXECUTE FUNCTION history.guard_restore_staging();
COMMENT ON FUNCTION history.guard_restore_staging() IS 'Protects request identity/TTL and verifies whole-object staging bytes. Application completion additionally fences membership, policy, lease and tenant budgets.';
