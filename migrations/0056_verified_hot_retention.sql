-- Explicit opt-in hot-payload lifecycle. Identities, source/binding FKs and immutable
-- archive membership remain. NULL is an all-or-none archived payload, never a new fact.
-- No worker or approval starts by migrating. No table rewrite/backfill is required;
-- The CHECK is NOT VALID: existing rows already satisfy all three NOT NULL rules.
-- This avoids a full history scan under the exclusive DDL lock. Future writes are
-- checked immediately; separately schedule validation if planner/catalog proof is needed.
-- Apply with lock_timeout and quiesced writers; lock acquisition can still wait.
-- Old binaries remain compatible until approval/execution; stop them before enabling.
ALTER TABLE history.asset_property_changes
    ALTER COLUMN event_at_ms DROP NOT NULL,
    ALTER COLUMN resolution_effective_at_ms DROP NOT NULL,
    ALTER COLUMN resolution_known_at_ms DROP NOT NULL,
    ADD CONSTRAINT ck_property_change_payload CHECK (
        (event_at_ms IS NOT NULL AND resolution_effective_at_ms IS NOT NULL AND resolution_known_at_ms IS NOT NULL)
        OR (event_at_ms IS NULL AND resolution_effective_at_ms IS NULL AND resolution_known_at_ms IS NULL)) NOT VALID;

-- Approval is a trusted operator action bound to a policy revision, separate from
-- configuring archive policy. Revocation/deactivation is checked again at commit.
CREATE TABLE history.retention_controls (
    tenant_id TEXT PRIMARY KEY REFERENCES metadata.tenants(tenant_id) ON DELETE NO ACTION,
    policy_revision BIGINT NOT NULL,
    actor_id TEXT NOT NULL REFERENCES identity.actors(actor_id) ON DELETE NO ACTION,
    FOREIGN KEY (tenant_id,policy_revision) REFERENCES history.lifecycle_policies(tenant_id,revision) ON DELETE NO ACTION
);
COMMENT ON TABLE history.retention_controls IS 'Explicit reviewed retention opt-in; absent means disabled. Private trusted operator access only, never operations.read authority.';

-- One all-member receipt per manifest. The transaction identity binds the only
-- permitted NULL transition to the same committing transaction as the audit.
CREATE TABLE history.retention_receipts (
    tenant_id TEXT NOT NULL,
    manifest_id BIGINT NOT NULL,
    policy_revision BIGINT NOT NULL,
    actor_id TEXT NOT NULL REFERENCES identity.actors(actor_id) ON DELETE NO ACTION,
    records INTEGER NOT NULL CHECK (records BETWEEN 1 AND 16),
    -- Historical admission ID is retained after the live reservation is released.
    reservation_id BIGINT NOT NULL CHECK (reservation_id>0),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    transaction_id XID8 NOT NULL DEFAULT pg_current_xact_id(),
    PRIMARY KEY (tenant_id,manifest_id),
    FOREIGN KEY (tenant_id,manifest_id) REFERENCES history.archive_jobs(tenant_id,job_id) ON DELETE NO ACTION,
    FOREIGN KEY (tenant_id,policy_revision) REFERENCES history.lifecycle_policies(tenant_id,revision) ON DELETE NO ACTION
);
COMMENT ON TABLE history.retention_receipts IS 'Immutable atomic hot-payload removal receipts; records*24 is removed logical field bytes, not net or physical savings.';
COMMENT ON COLUMN history.retention_receipts.transaction_id IS 'Only authorizes the narrow transition in the same transaction; not a reusable permit. Database owner remains trusted.';

-- Shared fail-closed dependency predicate. Caller must exclude writers to these
-- relations before evaluating: absence checks alone do not prevent phantom work.
CREATE FUNCTION history.retention_blocked(owner TEXT, manifest BIGINT, revision BIGINT)
RETURNS BOOLEAN LANGUAGE SQL VOLATILE AS $$
 SELECT NOT EXISTS(
   SELECT 1 FROM metadata.tenants t
   JOIN history.retention_controls c USING (tenant_id)
   JOIN identity.actors actor ON actor.actor_id=c.actor_id
   JOIN history.lifecycle_policies p ON p.tenant_id=t.tenant_id AND p.revision=c.policy_revision
   JOIN history.archive_jobs a ON a.tenant_id=t.tenant_id AND a.job_id=manifest
   WHERE t.tenant_id=owner AND t.is_active AND actor.is_active
     AND p.revision=revision AND p.revision=(SELECT max(revision) FROM history.lifecycle_policies WHERE tenant_id=owner)
     AND p.hot_days IS NOT NULL AND a.object_sha256 IS NOT NULL AND a.generation>0
     AND extract(epoch FROM clock_timestamp()-a.lease_until)/86400 >= p.hot_days::numeric)
 OR EXISTS(SELECT 1 FROM ingestion.replay_work WHERE tenant_id=owner AND status<>'completed')
 OR NOT EXISTS(SELECT 1 FROM history.semantic_backfill WHERE completed)
 OR EXISTS(SELECT 1 FROM history.semantic_work WHERE tenant_id=owner AND status<>'completed')
 OR EXISTS(SELECT 1 FROM analytics.calculator_schedules s LEFT JOIN analytics.calculator_configurations c USING (tenant_id,configuration_id)
   WHERE s.tenant_id=owner AND (c.configuration_id IS NULL OR c.active_revision IS NOT NULL))
 OR EXISTS(SELECT 1 FROM analytics.calculator_work WHERE tenant_id=owner AND status<>'done')
 OR EXISTS(SELECT 1 FROM analytics.prepared_evaluations p WHERE p.tenant_id=owner AND NOT EXISTS(
   SELECT 1 FROM analytics.evaluations e WHERE e.tenant_id=p.tenant_id AND e.evaluation_id=p.evaluation_id))
 OR EXISTS(SELECT 1 FROM analytics.evaluations e
   CROSS JOIN LATERAL jsonb_array_elements(e.replay_key->'inputs') input
   JOIN history.archive_members m ON m.tenant_id=e.tenant_id AND m.job_id=manifest
     AND analytics.semantic_evidence_id(m.tenant_id,m.message_id,m.protocol_package_id,m.protocol_package_version,m.output_index,m.binding_id,m.binding_revision,m.event_time_basis)=input->>0
   WHERE e.tenant_id=owner)
 OR EXISTS(SELECT 1 FROM history.restore_jobs WHERE tenant_id=owner AND
   ((status IN ('pending','ready') AND expires_at>clock_timestamp()) OR lease_until>clock_timestamp()))
 OR EXISTS(SELECT 1 FROM projection.pending_changes WHERE tenant_id=owner)
 OR EXISTS(SELECT 1 FROM history.archive_members m JOIN projection.current_properties c
   USING (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis)
   WHERE m.tenant_id=owner AND m.job_id=manifest)
$$;
COMMENT ON FUNCTION history.retention_blocked IS 'Conservative manifest eligibility; table write exclusion is required to serialize absence checks with new dependencies.';

-- Bounded executor transactions exclude phantom dependency/approval changes.
-- Coarse global relation locks deliberately favor correctness over throughput.
CREATE FUNCTION history.lock_retention_dependencies() RETURNS VOID LANGUAGE plpgsql AS $$
BEGIN
 IF current_setting('transaction_isolation')<>'read committed'
 THEN RAISE EXCEPTION 'retention requires read committed isolation' USING ERRCODE='25001'; END IF;
 LOCK TABLE history.asset_property_changes IN SHARE ROW EXCLUSIVE MODE;
 LOCK TABLE metadata.tenants,identity.actors,history.lifecycle_policies,history.retention_controls,
 ingestion.replay_work,history.semantic_backfill,history.semantic_work,
 analytics.calculator_schedules,analytics.calculator_configurations,analytics.calculator_work,
 analytics.prepared_evaluations,analytics.evaluations,history.restore_jobs,
 projection.current_properties,projection.pending_changes IN SHARE MODE;
END;
$$;
COMMENT ON FUNCTION history.lock_retention_dependencies IS 'Serializes bounded reclamation with history writers and every known dependency creator; caller must impose a short statement/lock timeout.';

CREATE FUNCTION history.check_retention_receipt() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 PERFORM history.lock_retention_dependencies();
 PERFORM 1 FROM history.work_reservations r WHERE r.tenant_id=NEW.tenant_id
   AND r.reservation_id=NEW.reservation_id AND r.policy_revision=NEW.policy_revision
   AND r.reserved_bytes>=2097152 AND r.expires_at>clock_timestamp() FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'retention reservation expired or changed'; END IF;
 IF NEW.transaction_id<>pg_current_xact_id()
    OR history.retention_blocked(NEW.tenant_id,NEW.manifest_id,NEW.policy_revision)
    OR NOT EXISTS(SELECT 1 FROM history.retention_controls WHERE tenant_id=NEW.tenant_id
      AND actor_id=NEW.actor_id AND policy_revision=NEW.policy_revision)
    OR NEW.records<>(SELECT count(*) FROM history.archive_members m JOIN history.asset_property_changes h
      USING (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis)
      WHERE m.tenant_id=NEW.tenant_id AND m.job_id=NEW.manifest_id AND h.event_at_ms IS NOT NULL)
 THEN RAISE EXCEPTION 'retention protection changed'; END IF;
 RETURN NEW;
END;
$$;
CREATE TRIGGER retention_receipt_checked BEFORE INSERT ON history.retention_receipts
 FOR EACH ROW EXECUTE FUNCTION history.check_retention_receipt();
COMMENT ON FUNCTION history.check_retention_receipt IS 'Enforces current explicit approval, consumer exclusion and exact bounded hot membership for every receipt insert. Host additionally verifies fresh complete provider bytes.';

-- Ordinary updates/deletes still fail. Only all three payload fields may become
-- NULL once, with an immutable same-transaction audited manifest receipt.
CREATE OR REPLACE FUNCTION history.prevent_property_change_mutation()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='UPDATE' AND OLD.event_at_ms IS NOT NULL AND NEW.event_at_ms IS NULL
    AND NEW.resolution_effective_at_ms IS NULL AND NEW.resolution_known_at_ms IS NULL
    AND (to_jsonb(NEW)-ARRAY['event_at_ms','resolution_effective_at_ms','resolution_known_at_ms'])
        = (to_jsonb(OLD)-ARRAY['event_at_ms','resolution_effective_at_ms','resolution_known_at_ms'])
    AND EXISTS(SELECT 1 FROM history.archive_members m JOIN history.retention_receipts r
        ON r.tenant_id=m.tenant_id AND r.manifest_id=m.job_id
        WHERE m.tenant_id=OLD.tenant_id AND m.message_id=OLD.message_id
          AND m.protocol_package_id=OLD.protocol_package_id AND m.protocol_package_version=OLD.protocol_package_version
          AND m.output_index=OLD.output_index AND m.binding_id=OLD.binding_id
          AND m.binding_revision=OLD.binding_revision AND m.event_time_basis=OLD.event_time_basis
          AND r.transaction_id=pg_current_xact_id()
          AND NOT history.retention_blocked(r.tenant_id,r.manifest_id,r.policy_revision))
 THEN RETURN NEW; END IF;
 RAISE EXCEPTION 'Semantic anchors and payload are immutable outside audited retention' USING ERRCODE='55000';
END;
$$;
COMMENT ON FUNCTION history.prevent_property_change_mutation IS 'Only same-transaction audited all-field NULL reclamation is allowed; anchor edits, deletes and rehydration remain forbidden.';

-- SQL NULL comparisons must never admit a new payload-less semantic fact.
CREATE FUNCTION history.require_new_property_payload() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.event_at_ms IS NULL OR NEW.resolution_effective_at_ms IS NULL OR NEW.resolution_known_at_ms IS NULL
 THEN RAISE EXCEPTION 'new semantic facts require complete payload'; END IF;
 RETURN NEW;
END;
$$;
CREATE TRIGGER asset_property_payload_required BEFORE INSERT ON history.asset_property_changes
 FOR EACH ROW EXECUTE FUNCTION history.require_new_property_payload();
COMMENT ON FUNCTION history.require_new_property_payload IS 'New/retried appends supply full facts; only existing archived rows can lose payload.';

CREATE FUNCTION history.retain_retention_receipt() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'retention receipts are immutable'; END;
$$;
CREATE TRIGGER retention_receipt_immutable BEFORE UPDATE OR DELETE ON history.retention_receipts
 FOR EACH ROW EXECUTE FUNCTION history.retain_retention_receipt();
COMMENT ON FUNCTION history.retain_retention_receipt IS 'Retains original audit and retry reconciliation evidence.';

COMMENT ON COLUMN history.asset_property_changes.event_at_ms IS 'Signed event time while hot; NULL with both resolution fields only after audited verified reclamation. Source facts and identity remain.';
COMMENT ON COLUMN history.asset_property_changes.resolution_effective_at_ms IS 'Original physical resolution cutoff while hot; archived original remains in verified object after reclamation.';
COMMENT ON COLUMN history.asset_property_changes.resolution_known_at_ms IS 'Original recorded resolution cutoff while hot; archived original remains in verified object after reclamation.';

-- Rebuild ignores archived payloads. Every current winner is protected from
-- reclamation, so excluding older archived losers preserves the same winner.
CREATE OR REPLACE FUNCTION projection.rebuild_current(selected_tenant TEXT) RETURNS VOID
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
    WHERE h.tenant_id=selected_tenant AND h.event_at_ms IS NOT NULL
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

-- A receipt without the matching complete transition must not commit, including
-- manual SQL or cancellation between receipt insertion and payload UPDATE.
CREATE FUNCTION history.check_retention_completed() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF NOT EXISTS(SELECT 1 FROM history.work_reservations r WHERE r.tenant_id=NEW.tenant_id
       AND r.reservation_id=NEW.reservation_id AND r.policy_revision=NEW.policy_revision
       AND r.expires_at>clock_timestamp())
 OR (SELECT count(*) FROM history.archive_members m
     JOIN history.asset_property_changes h
     USING (tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis)
     WHERE m.tenant_id=NEW.tenant_id AND m.job_id=NEW.manifest_id
       AND h.event_at_ms IS NULL) <> NEW.records
 THEN RAISE EXCEPTION 'retention receipt requires complete payload transition'; END IF;
 RETURN NULL;
END;
$$;
CREATE CONSTRAINT TRIGGER retention_receipt_completed AFTER INSERT ON history.retention_receipts
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION history.check_retention_completed();
COMMENT ON FUNCTION history.check_retention_completed IS 'Commit-time all-member completeness; interruption cannot leave an audit receipt without its exact payload transition.';


-- Reclaimed anchors must never turn SQL NULL comparisons into approval of new
-- arbitrary calculator evidence. Existing terminal references are pinned above.
CREATE FUNCTION analytics.require_hot_evaluation_evidence() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 -- Row locks make stale REPEATABLE READ/SERIALIZABLE writers abort rather than
 -- validating references using pre-reclamation payloads.
 IF jsonb_typeof(NEW.replay_key->'inputs')='array' AND jsonb_array_length(NEW.replay_key->'inputs')<=1024 THEN
   PERFORM 1 FROM jsonb_array_elements(NEW.replay_key->'inputs') input
     JOIN history.asset_property_changes h ON h.tenant_id=NEW.tenant_id
       AND analytics.semantic_evidence_id(h.tenant_id,h.message_id,h.protocol_package_id,h.protocol_package_version,h.output_index,h.binding_id,h.binding_revision,h.event_time_basis)=input->>0
     FOR SHARE OF h;
 END IF;
 IF jsonb_typeof(NEW.replay_key->'inputs')='array' AND jsonb_array_length(NEW.replay_key->'inputs')<=1024
    AND EXISTS(SELECT 1 FROM jsonb_array_elements(NEW.replay_key->'inputs') input
      JOIN history.asset_property_changes h ON h.tenant_id=NEW.tenant_id
       AND analytics.semantic_evidence_id(h.tenant_id,h.message_id,h.protocol_package_id,h.protocol_package_version,h.output_index,h.binding_id,h.binding_revision,h.event_time_basis)=input->>0
      WHERE h.event_at_ms IS NULL)
 THEN RAISE EXCEPTION 'archived calculator evidence requires an explicit supported preparation'; END IF;
 RETURN NEW;
END;
$$;
CREATE TRIGGER evaluations_hot_evidence BEFORE INSERT ON analytics.evaluations
 FOR EACH ROW EXECUTE FUNCTION analytics.require_hot_evaluation_evidence();
COMMENT ON FUNCTION analytics.require_hot_evaluation_evidence IS 'Rejects new calculator references to archived payloads; existing terminal evaluation references remain retention pins.';


-- Current/pending pointers also cannot be created from a stale hot snapshot.
CREATE FUNCTION projection.require_hot_change_reference() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE hot BIGINT;
BEGIN
 SELECT h.event_at_ms INTO hot FROM history.asset_property_changes h
 WHERE (h.tenant_id,h.message_id,h.protocol_package_id,h.protocol_package_version,h.output_index,h.binding_id,h.binding_revision,h.event_time_basis)
   =(NEW.tenant_id,NEW.message_id,NEW.protocol_package_id,NEW.protocol_package_version,NEW.output_index,NEW.binding_id,NEW.binding_revision,NEW.event_time_basis)
 FOR SHARE;
 IF NOT FOUND OR hot IS NULL THEN RAISE EXCEPTION 'projection requires hot original evidence'; END IF;
 RETURN NEW;
END;
$$;
CREATE TRIGGER current_properties_hot BEFORE INSERT OR UPDATE ON projection.current_properties
 FOR EACH ROW EXECUTE FUNCTION projection.require_hot_change_reference();
CREATE TRIGGER pending_changes_hot BEFORE INSERT OR UPDATE ON projection.pending_changes
 FOR EACH ROW EXECUTE FUNCTION projection.require_hot_change_reference();
COMMENT ON FUNCTION projection.require_hot_change_reference IS 'Locks the exact source row so stale snapshots cannot publish pointers to reclaimed payloads.';


-- Preparations are immutable first snapshots. A stale transaction must not
-- publish a preparation that could never pass terminal hot-evidence validation.
CREATE FUNCTION analytics.require_current_preparation() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF current_setting('transaction_isolation')<>'read committed'
 THEN RAISE EXCEPTION 'calculator preparation requires read committed isolation' USING ERRCODE='25001'; END IF;
 RETURN NEW;
END;
$$;
CREATE TRIGGER prepared_evaluations_current BEFORE INSERT ON analytics.prepared_evaluations
 FOR EACH ROW EXECUTE FUNCTION analytics.require_current_preparation();
COMMENT ON FUNCTION analytics.require_current_preparation IS 'Requires READ COMMITTED first-snapshot writes; host preparation pins that isolation explicitly.';
