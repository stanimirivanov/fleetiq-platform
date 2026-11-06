-- Additive derived numeric snapshots. No worker, raw-history rewrite or implicit
-- policy activation. Each exact target/window/restore-selection/algorithm owns a
-- serialized revision head; original evidence and older revisions remain immutable.
CREATE TABLE history.numeric_summary_series (
 tenant_id TEXT NOT NULL,
 series_id BIGINT GENERATED ALWAYS AS IDENTITY,
 asset_id TEXT NOT NULL,
 property_id TEXT NOT NULL,
 property_version BIGINT NOT NULL,
 start_ms BIGINT NOT NULL,
 end_ms BIGINT NOT NULL,
 width_ms BIGINT NOT NULL CHECK(width_ms>0),
 algorithm INTEGER NOT NULL DEFAULT 1 CHECK(algorithm=1),
 manifest_id BIGINT,
 restore_generation BIGINT,
 revision BIGINT NOT NULL DEFAULT 0 CHECK(revision>=0),
 CONSTRAINT pk_numeric_summary_series PRIMARY KEY(tenant_id,series_id),
 CONSTRAINT uq_numeric_summary_scope UNIQUE NULLS NOT DISTINCT(tenant_id,asset_id,property_id,property_version,start_ms,end_ms,width_ms,algorithm,manifest_id,restore_generation),
 CONSTRAINT fk_numeric_summary_asset FOREIGN KEY(tenant_id,asset_id) REFERENCES metadata.assets(tenant_id,asset_id) ON DELETE NO ACTION,
 CONSTRAINT fk_numeric_summary_property FOREIGN KEY(tenant_id,property_id,property_version) REFERENCES metadata.property_definitions(tenant_id,property_id,version) ON DELETE NO ACTION,
 CONSTRAINT fk_numeric_summary_archive FOREIGN KEY(tenant_id,manifest_id) REFERENCES history.archive_jobs(tenant_id,job_id) ON DELETE NO ACTION,
 CONSTRAINT ck_numeric_summary_window CHECK(end_ms::numeric-start_ms BETWEEN 1 AND 604800000 AND mod(start_ms::numeric,width_ms)=0 AND mod(end_ms::numeric,width_ms)=0 AND (end_ms::numeric-start_ms)/width_ms<=64),
 CONSTRAINT ck_numeric_summary_restore CHECK((manifest_id IS NULL AND restore_generation IS NULL) OR (manifest_id IS NOT NULL AND manifest_id>0 AND restore_generation IS NOT NULL AND restore_generation>0))
);
COMMENT ON TABLE history.numeric_summary_series IS 'Exact target/window/epoch-aligned resolution and explicit restore selection; revision is publication order, never source completeness or freshness.';
CREATE TABLE history.numeric_summaries (
 tenant_id TEXT NOT NULL,
 series_id BIGINT NOT NULL,
 revision BIGINT NOT NULL CHECK(revision>0),
 observed_at_ms BIGINT NOT NULL,
 reservation_id BIGINT NOT NULL CHECK(reservation_id>0),
 body JSONB NOT NULL CHECK(jsonb_typeof(body)='object' AND octet_length(body::text)<=1048576
   AND body ?& ARRAY['algorithm','inputs','buckets','coverage','kind','unit'] AND (body->>'algorithm') IS NOT DISTINCT FROM 'numeric-fixed-v1' AND jsonb_typeof(body->'inputs')='array' AND jsonb_array_length(body->'inputs')<=100
   AND jsonb_typeof(body->'buckets')='array' AND jsonb_array_length(body->'buckets') BETWEEN 1 AND 64),
 fingerprint BYTEA NOT NULL CHECK(fingerprint=sha256(convert_to(body::text,'UTF8'))),
 CONSTRAINT pk_numeric_summaries PRIMARY KEY(tenant_id,series_id,revision),
 CONSTRAINT uq_numeric_summary_content UNIQUE(tenant_id,series_id,fingerprint),
 CONSTRAINT fk_numeric_summary_series FOREIGN KEY(tenant_id,series_id) REFERENCES history.numeric_summary_series(tenant_id,series_id) ON DELETE NO ACTION
);
COMMENT ON TABLE history.numeric_summaries IS 'Immutable exact derived evidence; body contains normalized numeric strings, input lineage, quality counts and snapshot coverage. Mean is sum/count, undefined at count zero.';
COMMENT ON COLUMN history.numeric_summaries.reservation_id IS 'Historical admission ID; the live tenant reservation must survive publication commit and is released afterwards.';
-- The same complete admission predicate is locked at insert and rechecked at commit.
CREATE FUNCTION history.numeric_summary_admitted(owner TEXT, reservation BIGINT) RETURNS BOOLEAN LANGUAGE plpgsql AS $$
BEGIN
 PERFORM 1 FROM history.work_reservations r JOIN metadata.tenants t USING(tenant_id)
 JOIN history.lifecycle_policies p ON p.tenant_id=r.tenant_id AND p.revision=r.policy_revision
 WHERE r.tenant_id=owner AND r.reservation_id=reservation AND r.reserved_bytes>=2097152
 AND r.expires_at>clock_timestamp() AND t.is_active AND p.max_jobs IS NOT NULL AND p.max_bytes>=2097152
 AND p.revision=(SELECT max(revision) FROM history.lifecycle_policies WHERE tenant_id=owner)
 FOR UPDATE OF r;
 RETURN FOUND;
END; $$;
COMMENT ON FUNCTION history.numeric_summary_admitted IS 'Locks the exact enabled/current-policy reservation; commit repeats all tenant, amount, policy and lifetime predicates.';

-- Validate durable v1 structure against its exact series. Arithmetic remains in
-- the bounded exact host reducer; SQL enforces scope, bucket geometry and counts.
CREATE FUNCTION history.check_numeric_summary_body(owner TEXT, series BIGINT, document JSONB) RETURNS VOID LANGUAGE plpgsql AS $$
DECLARE scope history.numeric_summary_series; kind TEXT; unit TEXT; bucket JSONB; sample JSONB; ordinal BIGINT;
 expected_start BIGINT; expected_end BIGINT; total_count INTEGER:=0; good_count INTEGER; all_count INTEGER; number_pattern TEXT:='^-?(0|[1-9][0-9]*)([.][0-9]*[1-9])?$';
BEGIN
 SELECT * INTO STRICT scope FROM history.numeric_summary_series WHERE tenant_id=owner AND series_id=series;
 SELECT value_kind,canonical_unit INTO STRICT kind,unit FROM metadata.property_definitions WHERE tenant_id=owner AND property_id=scope.property_id AND version=scope.property_version;
 IF kind NOT IN ('integer','decimal') OR document->>'kind' IS DISTINCT FROM kind
 OR document->'unit' IS DISTINCT FROM COALESCE(to_jsonb(unit),'null'::jsonb)
 OR document-ARRAY['algorithm','kind','unit','coverage','inputs','buckets']<>'{}'::jsonb
 OR document->'coverage' IS DISTINCT FROM jsonb_build_object('scope',CASE WHEN scope.manifest_id IS NULL THEN 'hot_snapshot' ELSE 'hot_and_selected_manifest_snapshot' END,'manifest_id',scope.manifest_id::text,'restore_generation',scope.restore_generation::text)
 OR jsonb_array_length(document->'buckets')<>(scope.end_ms::numeric-scope.start_ms)/scope.width_ms
 THEN RAISE EXCEPTION 'numeric summary scope/body mismatch'; END IF;
 IF EXISTS(SELECT 1 FROM jsonb_array_elements(document->'inputs') x
   WHERE jsonb_typeof(x) IS DISTINCT FROM 'object' OR NOT x ?& ARRAY['id','event_at_ms','value','quality']
     OR x-ARRAY['id','event_at_ms','value','quality']<>'{}'::jsonb
     OR jsonb_typeof(x->'id') IS DISTINCT FROM 'string' OR length(x->>'id') NOT BETWEEN 1 AND 8192
     OR jsonb_typeof(x->'event_at_ms') IS DISTINCT FROM 'string'
     OR (x->>'event_at_ms')::bigint<scope.start_ms OR (x->>'event_at_ms')::bigint>=scope.end_ms
     OR jsonb_typeof(x->'value') IS DISTINCT FROM 'string' OR length(x->>'value')>2048 OR NOT (x->>'value')~number_pattern OR (kind='integer' AND x->>'value' LIKE '%.%')
     OR jsonb_typeof(x->'quality') IS DISTINCT FROM 'string' OR x->>'quality' NOT IN ('good','uncertain','invalid'))
 OR (SELECT count(DISTINCT x->>'id') FROM jsonb_array_elements(document->'inputs') x)<>jsonb_array_length(document->'inputs')
 THEN RAISE EXCEPTION 'invalid numeric summary input structure'; END IF;
 FOR bucket,ordinal IN SELECT value,ordinality FROM jsonb_array_elements(document->'buckets') WITH ORDINALITY LOOP
  expected_start:=scope.start_ms+(ordinal-1)*scope.width_ms; expected_end:=expected_start+scope.width_ms;
  IF jsonb_typeof(bucket) IS DISTINCT FROM 'object' OR NOT bucket ?& ARRAY['start_ms','end_ms','total','count','excluded','min','max','sum']
   OR bucket-ARRAY['start_ms','end_ms','total','count','excluded','min','max','sum']<>'{}'::jsonb
   OR bucket->'start_ms' IS DISTINCT FROM to_jsonb(expected_start::text) OR bucket->'end_ms' IS DISTINCT FROM to_jsonb(expected_end::text)
   OR jsonb_typeof(bucket->'total') IS DISTINCT FROM 'number' OR jsonb_typeof(bucket->'count') IS DISTINCT FROM 'number' OR jsonb_typeof(bucket->'excluded') IS DISTINCT FROM 'number'
   OR (bucket->>'total')::integer NOT BETWEEN 0 AND 100 OR (bucket->>'count')::integer NOT BETWEEN 0 AND 100 OR (bucket->>'excluded')::integer NOT BETWEEN 0 AND 100
   OR (bucket->>'total')::integer<>(bucket->>'count')::integer+(bucket->>'excluded')::integer
   OR jsonb_typeof(bucket->'sum') IS DISTINCT FROM 'string' OR length(bucket->>'sum')>2048 OR NOT (bucket->>'sum')~number_pattern OR (kind='integer' AND bucket->>'sum' LIKE '%.%')
  THEN RAISE EXCEPTION 'invalid numeric bucket structure'; END IF;
  SELECT count(*),count(*) FILTER(WHERE x->>'quality'='good') INTO all_count,good_count FROM jsonb_array_elements(document->'inputs') x
   WHERE (x->>'event_at_ms')::bigint>=expected_start AND (x->>'event_at_ms')::bigint<expected_end;
  IF (bucket->>'total')::integer<>all_count OR (bucket->>'count')::integer<>good_count
  THEN RAISE EXCEPTION 'numeric bucket counts disagree with lineage'; END IF;
  IF good_count=0 THEN
   IF bucket->'min' IS DISTINCT FROM 'null'::jsonb OR bucket->'max' IS DISTINCT FROM 'null'::jsonb OR bucket->>'sum' IS DISTINCT FROM '0'
   THEN RAISE EXCEPTION 'empty numeric bucket has statistics'; END IF;
  ELSE
   IF jsonb_typeof(bucket->'min') IS DISTINCT FROM 'string' OR jsonb_typeof(bucket->'max') IS DISTINCT FROM 'string'
    OR length(bucket->>'min')>2048 OR length(bucket->>'max')>2048 OR NOT (bucket->>'min')~number_pattern OR NOT (bucket->>'max')~number_pattern
    OR (bucket->>'min')::numeric>(bucket->>'max')::numeric OR (kind='integer' AND (bucket->>'min' LIKE '%.%' OR bucket->>'max' LIKE '%.%'))
   THEN RAISE EXCEPTION 'invalid numeric extrema'; END IF;
  END IF;
  total_count:=total_count+all_count;
 END LOOP;
 IF total_count<>jsonb_array_length(document->'inputs') THEN RAISE EXCEPTION 'numeric lineage count mismatch'; END IF;
END; $$;
COMMENT ON FUNCTION history.check_numeric_summary_body IS 'Enforces v1 typed scope, complete bucket geometry, quality/count lineage, exact numeric string grammar and explicit coverage.';

-- Head changes cannot rewrite scope or skip a publication. New records require
-- the matching next head and a live current-policy allowance under the tenant lock.
CREATE FUNCTION history.guard_numeric_summary() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE current_revision BIGINT;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'numeric summary evidence is immutable'; END IF;
 IF current_setting('transaction_isolation')<>'read committed'
 THEN RAISE EXCEPTION 'numeric publication requires read committed isolation' USING ERRCODE='25001'; END IF;
 PERFORM 1 FROM metadata.tenants WHERE tenant_id=NEW.tenant_id AND is_active FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'numeric publication requires active tenant'; END IF;
 IF TG_TABLE_NAME='numeric_summary_series' THEN
  IF NOT EXISTS(SELECT 1 FROM metadata.assets a JOIN metadata.asset_type_properties p
    ON p.tenant_id=a.tenant_id AND p.type_id=a.type_id AND p.type_version=a.type_version
    WHERE a.tenant_id=NEW.tenant_id AND a.asset_id=NEW.asset_id AND p.property_id=NEW.property_id AND p.property_version=NEW.property_version)
  THEN RAISE EXCEPTION 'numeric summary target is not installed'; END IF;
  IF NEW.manifest_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM history.archive_jobs WHERE tenant_id=NEW.tenant_id AND job_id=NEW.manifest_id AND object_sha256 IS NOT NULL)
  THEN RAISE EXCEPTION 'numeric summary selection requires a published manifest'; END IF;
  IF TG_OP='INSERT' THEN IF NEW.revision<>0 THEN RAISE EXCEPTION 'new summary series starts at zero'; END IF; RETURN NEW; END IF;
  IF TG_OP<>'UPDATE' OR (to_jsonb(NEW)-'revision') IS DISTINCT FROM (to_jsonb(OLD)-'revision')
     OR NEW.revision<>OLD.revision+1 OR NOT EXISTS(SELECT 1 FROM history.numeric_summaries WHERE tenant_id=NEW.tenant_id AND series_id=NEW.series_id AND revision=NEW.revision)
  THEN RAISE EXCEPTION 'numeric summary scope/head is immutable outside publication'; END IF;
 ELSIF TG_OP='INSERT' THEN
  SELECT revision INTO current_revision FROM history.numeric_summary_series WHERE tenant_id=NEW.tenant_id AND series_id=NEW.series_id FOR UPDATE;
  IF current_revision IS NULL OR NEW.revision<>current_revision+1 THEN RAISE EXCEPTION 'numeric summary revision is not next'; END IF;
  IF NOT history.numeric_summary_admitted(NEW.tenant_id,NEW.reservation_id)
  THEN RAISE EXCEPTION 'numeric summary admission changed'; END IF;
  PERFORM history.check_numeric_summary_body(NEW.tenant_id,NEW.series_id,NEW.body);
  IF current_revision>0 AND NOT (NEW.body->'inputs') @> (SELECT body->'inputs' FROM history.numeric_summaries WHERE tenant_id=NEW.tenant_id AND series_id=NEW.series_id AND revision=current_revision)
  THEN RAISE EXCEPTION 'numeric summary snapshot drops previously published evidence'; END IF;
 ELSE RAISE EXCEPTION 'numeric summary revisions are immutable';
 END IF;
 RETURN NEW;
END; $$;
CREATE TRIGGER numeric_summary_head_guard BEFORE INSERT OR UPDATE OR DELETE ON history.numeric_summary_series FOR EACH ROW EXECUTE FUNCTION history.guard_numeric_summary();
CREATE TRIGGER numeric_summary_guard BEFORE INSERT OR UPDATE OR DELETE ON history.numeric_summaries FOR EACH ROW EXECUTE FUNCTION history.guard_numeric_summary();
COMMENT ON FUNCTION history.guard_numeric_summary IS 'Preserves exact scope and immutable revisions; validates serialized next publication and current live tenant work allowance.';
CREATE FUNCTION history.check_numeric_summary_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF NOT EXISTS(SELECT 1 FROM history.numeric_summary_series WHERE tenant_id=NEW.tenant_id AND series_id=NEW.series_id AND revision>=NEW.revision)
 OR NOT history.numeric_summary_admitted(NEW.tenant_id,NEW.reservation_id)
 THEN RAISE EXCEPTION 'numeric summary publication did not complete'; END IF;
 RETURN NULL;
END; $$;
CREATE CONSTRAINT TRIGGER numeric_summary_completed AFTER INSERT ON history.numeric_summaries DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION history.check_numeric_summary_commit();
COMMENT ON FUNCTION history.check_numeric_summary_commit IS 'Commit-time head/admission fence; interrupted publication cannot leave a usable partial revision.';
