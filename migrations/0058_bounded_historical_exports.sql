-- Temporary derived artifacts; immutable selected generation and independent worker attempts.
-- No worker, policy activation, source rewrite or existing-table backfill.
CREATE TABLE history.export_jobs (
 tenant_id TEXT NOT NULL,
 job_id BIGINT GENERATED ALWAYS AS IDENTITY,
 request_key TEXT NOT NULL CHECK(request_key ~ '^[a-z0-9][a-z0-9._-]{0,127}$'),
 asset_id TEXT NOT NULL,
 property_id TEXT NOT NULL,
 property_version BIGINT NOT NULL,
 start_ms BIGINT NOT NULL,
 end_ms BIGINT NOT NULL CHECK(end_ms::numeric-start_ms BETWEEN 1 AND 604800000),
 manifest_id BIGINT,
 restore_generation BIGINT,
 selection_generation INTEGER NOT NULL DEFAULT 1 CHECK(selection_generation=1),
 format_version INTEGER NOT NULL DEFAULT 1 CHECK(format_version=1),
 row_count INTEGER NOT NULL CHECK(row_count BETWEEN 1 AND 16),
 payload BYTEA,
 fingerprint BYTEA NOT NULL CHECK(octet_length(fingerprint)=32),
 file_bytes BIGINT NOT NULL CHECK(file_bytes BETWEEN 1 AND 2097152),
 admitted_by BIGINT NOT NULL CHECK(admitted_by>0),
 admission_until TIMESTAMPTZ NOT NULL,
 status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','ready','cancelled','expired')),
 attempt BIGINT NOT NULL DEFAULT 0 CHECK(attempt>=0),
 lease_until TIMESTAMPTZ,
 expires_at TIMESTAMPTZ NOT NULL DEFAULT(clock_timestamp()+interval '1 hour'),
 CONSTRAINT pk_export_jobs PRIMARY KEY(tenant_id,job_id),
 CONSTRAINT uq_export_request UNIQUE(tenant_id,request_key),
 CONSTRAINT uq_export_admission UNIQUE(tenant_id,admitted_by),
 CONSTRAINT fk_export_asset FOREIGN KEY(tenant_id,asset_id) REFERENCES metadata.assets(tenant_id,asset_id),
 CONSTRAINT fk_export_property FOREIGN KEY(tenant_id,property_id,property_version) REFERENCES metadata.property_definitions(tenant_id,property_id,version),
 CONSTRAINT fk_export_manifest FOREIGN KEY(tenant_id,manifest_id) REFERENCES history.archive_jobs(tenant_id,job_id),
 CONSTRAINT ck_export_selection CHECK((manifest_id IS NULL AND restore_generation IS NULL) OR (manifest_id IS NOT NULL AND manifest_id>0 AND restore_generation IS NOT NULL AND restore_generation>0)),
 CONSTRAINT ck_export_payload CHECK((status IN ('pending','ready') AND payload IS NOT NULL AND octet_length(payload)=file_bytes AND sha256(payload)=fingerprint) OR (status IN ('cancelled','expired') AND payload IS NULL))
);
CREATE INDEX ix_export_live ON history.export_jobs(tenant_id,expires_at) WHERE status IN ('pending','ready');
CREATE INDEX ix_export_leases ON history.export_jobs(tenant_id,lease_until) WHERE lease_until IS NOT NULL;
CREATE INDEX ix_export_expiry ON history.export_jobs(tenant_id,expires_at,job_id) WHERE payload IS NOT NULL;
COMMENT ON TABLE history.export_jobs IS 'Explicit bounded property-history-v1 exports. Frozen file is the selected evidence generation; ready publishes identical verified bytes. Expired/cancelled copies are unreadable; keys/receipts never reopen.';
COMMENT ON COLUMN history.export_jobs.admitted_by IS 'Historical creation reservation, atomically replaced by this job slot and conservative 4 MiB charge.';
COMMENT ON COLUMN history.export_jobs.lease_until IS 'Thirty-second attempt fence; cancelled/expired work retains quota while its owner may still execute.';

CREATE FUNCTION history.export_slots(owner TEXT) RETURNS BIGINT LANGUAGE SQL STABLE AS $$
 SELECT (SELECT count(*) FROM history.export_jobs WHERE tenant_id=owner AND status IN ('pending','ready') AND expires_at>statement_timestamp())
 + (SELECT count(*) FROM history.export_jobs WHERE tenant_id=owner AND lease_until>statement_timestamp()
    AND (status NOT IN ('pending','ready') OR expires_at<=statement_timestamp()))
$$;
CREATE FUNCTION history.export_bytes(owner TEXT) RETURNS NUMERIC LANGUAGE SQL STABLE AS $$
 SELECT history.export_slots(owner)::numeric*4194304
$$;
COMMENT ON FUNCTION history.export_slots IS 'Live selections/artifacts and still-running terminated attempts each consume one shared tenant job slot.';
COMMENT ON FUNCTION history.export_bytes IS 'Conservative logical work/staging allowance, not physical storage or measured RSS.';

CREATE FUNCTION history.export_admitted(owner TEXT) RETURNS BOOLEAN LANGUAGE SQL STABLE AS $$
 SELECT COALESCE((SELECT t.is_active AND p.max_jobs IS NOT NULL
 AND history.export_slots(owner)+(SELECT count(*) FROM history.work_reservations WHERE tenant_id=owner AND expires_at>clock_timestamp())<=p.max_jobs
 AND history.export_bytes(owner)+(SELECT COALESCE(sum(reserved_bytes::numeric),0) FROM history.work_reservations WHERE tenant_id=owner AND expires_at>clock_timestamp())
 +(SELECT COALESCE(sum(octet_length(payload)::numeric),0) FROM history.restore_jobs WHERE tenant_id=owner AND status='ready' AND expires_at>clock_timestamp())<=p.max_bytes::numeric
 FROM metadata.tenants t JOIN history.lifecycle_policies p ON p.tenant_id=t.tenant_id
 WHERE t.tenant_id=owner ORDER BY p.revision DESC LIMIT 1),false)
$$;
COMMENT ON FUNCTION history.export_admitted IS 'Complete current-policy shared slot/byte accounting. Caller holds the tenant row under READ COMMITTED.';

CREATE FUNCTION history.guard_export_job() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'export identities are immutable'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION 'exports require read committed publication' USING ERRCODE='25001'; END IF;
 PERFORM 1 FROM metadata.tenants WHERE tenant_id=NEW.tenant_id FOR UPDATE;
 IF TG_OP='INSERT' THEN
  IF NEW.status<>'pending' OR NEW.attempt<>0 OR NEW.lease_until IS NOT NULL
   OR NEW.expires_at<=clock_timestamp() OR NEW.expires_at>clock_timestamp()+interval '1 hour'
   OR NOT history.export_admitted(NEW.tenant_id)
   OR NOT EXISTS(SELECT 1 FROM history.work_reservations r WHERE r.tenant_id=NEW.tenant_id AND r.reservation_id=NEW.admitted_by AND r.reserved_bytes>=4194304 AND r.expires_at>clock_timestamp() AND r.policy_revision=(SELECT max(revision) FROM history.lifecycle_policies WHERE tenant_id=NEW.tenant_id))
  THEN RAISE EXCEPTION 'export requires current admission'; END IF;
  SELECT expires_at INTO NEW.admission_until FROM history.work_reservations WHERE tenant_id=NEW.tenant_id AND reservation_id=NEW.admitted_by FOR UPDATE;
  IF NOT EXISTS(SELECT 1 FROM metadata.assets a JOIN metadata.asset_type_properties p ON p.tenant_id=a.tenant_id AND p.type_id=a.type_id AND p.type_version=a.type_version WHERE a.tenant_id=NEW.tenant_id AND a.asset_id=NEW.asset_id AND p.property_id=NEW.property_id AND p.property_version=NEW.property_version)
  THEN RAISE EXCEPTION 'export target is not installed'; END IF;
  RETURN NEW;
 END IF;
 IF (to_jsonb(NEW)-ARRAY['status','attempt','lease_until','payload','expires_at']) IS DISTINCT FROM (to_jsonb(OLD)-ARRAY['status','attempt','lease_until','payload','expires_at'])
 OR NEW.expires_at>OLD.expires_at OR (NEW.payload IS NOT NULL AND NEW.payload IS DISTINCT FROM OLD.payload)
 OR (OLD.status IN ('cancelled','expired') AND NEW.status NOT IN ('cancelled','expired'))
 OR (OLD.status='ready' AND NEW.status='pending')
 THEN RAISE EXCEPTION 'export selection/lifetime is immutable'; END IF;
 IF OLD.status IN ('pending','ready') AND NEW.status IN ('cancelled','expired') AND NEW.lease_until IS DISTINCT FROM OLD.lease_until
 THEN RAISE EXCEPTION 'termination must preserve running export ownership'; END IF;
 IF NEW.attempt<>OLD.attempt THEN
  IF OLD.status<>'pending' OR NEW.status<>'pending' OR NEW.attempt<>OLD.attempt+1
   OR OLD.lease_until>clock_timestamp() OR NEW.lease_until IS NULL OR NEW.lease_until<=clock_timestamp()
   OR NEW.lease_until>clock_timestamp()+interval '30 seconds' OR NEW.expires_at<=clock_timestamp()+interval '20 seconds'
   OR NOT history.export_admitted(NEW.tenant_id)
  THEN RAISE EXCEPTION 'invalid export attempt'; END IF;
 ELSIF NEW.lease_until IS DISTINCT FROM OLD.lease_until AND NEW.lease_until IS NOT NULL
  AND (OLD.lease_until IS NULL OR NEW.lease_until>OLD.lease_until)
 THEN RAISE EXCEPTION 'export lease cannot extend'; END IF;
 IF OLD.status='pending' AND NEW.status='ready' THEN
  IF OLD.lease_until IS NULL OR OLD.lease_until<=clock_timestamp() OR NEW.expires_at<=clock_timestamp()
   OR NEW.attempt<>OLD.attempt OR NOT history.export_admitted(NEW.tenant_id)
  THEN RAISE EXCEPTION 'export publication fenced'; END IF;
 END IF;
 IF NEW.status='expired' AND NEW.expires_at>clock_timestamp() THEN RAISE EXCEPTION 'unexpired export'; END IF;
 RETURN NEW;
END; $$;
CREATE TRIGGER export_job_guard BEFORE INSERT OR UPDATE OR DELETE ON history.export_jobs FOR EACH ROW EXECUTE FUNCTION history.guard_export_job();
COMMENT ON FUNCTION history.guard_export_job IS 'Freezes scope, bytes, generation and bounded lifetime; fences publication, retries and terminal states under the tenant lock.';
CREATE FUNCTION history.check_export_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='INSERT' AND (NEW.admission_until<=clock_timestamp() OR EXISTS(SELECT 1 FROM history.work_reservations WHERE tenant_id=NEW.tenant_id AND reservation_id=NEW.admitted_by))
 THEN RAISE EXCEPTION 'export admission transfer did not complete'; END IF;
 IF TG_OP='UPDATE' AND OLD.status='pending' AND NEW.status='ready' AND (OLD.lease_until IS NULL OR OLD.lease_until<=clock_timestamp() OR NEW.expires_at<=clock_timestamp())
 THEN RAISE EXCEPTION 'export attempt expired before commit'; END IF;
 IF (TG_OP='INSERT' OR (OLD.status='pending' AND NEW.status='ready') OR NEW.attempt>OLD.attempt)
 AND EXISTS(SELECT 1 FROM history.export_jobs WHERE tenant_id=NEW.tenant_id AND job_id=NEW.job_id AND status IN ('pending','ready') AND expires_at>clock_timestamp())
 AND NOT history.export_admitted(NEW.tenant_id)
 THEN RAISE EXCEPTION 'export shared quota changed before commit'; END IF;
 RETURN NULL;
END; $$;
CREATE CONSTRAINT TRIGGER export_job_completed AFTER INSERT OR UPDATE ON history.export_jobs DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION history.check_export_commit();
COMMENT ON FUNCTION history.check_export_commit IS 'Checks the final shared allowance after atomic reservation-to-job transfer. Trusted writers keep this constraint deferred until commit.';
