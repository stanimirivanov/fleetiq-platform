-- ============================================================================
-- Attributed relationship and signal-binding revisions
-- ============================================================================
-- The immutable fact already owns recorded time and effective interval. These
-- write-once records add the original authorized actor to exact fact identities.
-- Absence means historical attribution is unknown; no legacy row is backfilled.
-- Empty additive tables/indexes require no rewrite. Foreign-key installation
-- briefly locks referenced tables. Retain this migration and facts on rollback.
-- Revision NUMERIC(20,0) matches the existing positive-u64 fact key exactly.

-- Directed graph fact attribution, independent of later corrections.
CREATE TABLE metadata.relationship_revision_records (
    tenant_id TEXT NOT NULL,
    relationship_id TEXT NOT NULL,
    revision NUMERIC(20,0) NOT NULL,
    actor_id TEXT NOT NULL,
    CONSTRAINT pk_relationship_revision_records PRIMARY KEY (tenant_id, relationship_id, revision),
    -- The composite exact-fact reference prevents orphan/cross-tenant evidence.
    CONSTRAINT fk_relationship_revision_records_fact
        FOREIGN KEY (tenant_id, relationship_id, revision)
        REFERENCES metadata.asset_relationship_revisions (tenant_id, relationship_id, revision) ON DELETE RESTRICT,
    CONSTRAINT fk_relationship_revision_records_actor
        FOREIGN KEY (actor_id) REFERENCES identity.actors (actor_id) ON DELETE RESTRICT
);
-- Supports actor-reference maintenance without scanning every recorded fact.
CREATE INDEX ix_relationship_revision_records_actor ON metadata.relationship_revision_records (actor_id);
COMMENT ON TABLE metadata.relationship_revision_records IS
    'Original authorized actor committed atomically with one immutable relationship revision; no legacy attribution is inferred.';
COMMENT ON COLUMN metadata.relationship_revision_records.tenant_id IS 'Owning tenant; part of the exact immutable fact reference.';
COMMENT ON COLUMN metadata.relationship_revision_records.relationship_id IS 'Stable relationship identity, scoped by tenant.';
COMMENT ON COLUMN metadata.relationship_revision_records.revision IS 'Exact positive-u64 revision number; the referenced immutable fact owns recorded time and physical interval.';
COMMENT ON COLUMN metadata.relationship_revision_records.actor_id IS 'First authenticated actor with exact tenant metadata.manage; retries never reattribute.';

-- Source-to-property fact attribution, independent of later corrections.
CREATE TABLE metadata.signal_binding_revision_records (
    tenant_id TEXT NOT NULL,
    binding_id TEXT NOT NULL,
    revision NUMERIC(20,0) NOT NULL,
    actor_id TEXT NOT NULL,
    CONSTRAINT pk_signal_binding_revision_records PRIMARY KEY (tenant_id, binding_id, revision),
    -- The composite exact-fact reference prevents orphan/cross-tenant evidence.
    CONSTRAINT fk_signal_binding_revision_records_fact
        FOREIGN KEY (tenant_id, binding_id, revision)
        REFERENCES metadata.signal_binding_revisions (tenant_id, binding_id, revision) ON DELETE RESTRICT,
    CONSTRAINT fk_signal_binding_revision_records_actor
        FOREIGN KEY (actor_id) REFERENCES identity.actors (actor_id) ON DELETE RESTRICT
);
-- Supports actor-reference maintenance without scanning every recorded fact.
CREATE INDEX ix_signal_binding_revision_records_actor ON metadata.signal_binding_revision_records (actor_id);
COMMENT ON TABLE metadata.signal_binding_revision_records IS
    'Original authorized actor committed atomically with one immutable binding revision; no legacy attribution is inferred.';
COMMENT ON COLUMN metadata.signal_binding_revision_records.tenant_id IS 'Owning tenant; part of the exact immutable fact reference.';
COMMENT ON COLUMN metadata.signal_binding_revision_records.binding_id IS 'Stable binding identity, scoped by tenant.';
COMMENT ON COLUMN metadata.signal_binding_revision_records.revision IS 'Exact positive-u64 revision number; the referenced immutable fact owns recorded time and physical interval.';
COMMENT ON COLUMN metadata.signal_binding_revision_records.actor_id IS 'First authenticated actor with exact tenant metadata.manage; retries never reattribute.';

-- Shared by both attribution tables; no retry or maintenance write may replace
-- original evidence. Statement-level TRUNCATE protection also covers empty tables.
CREATE FUNCTION metadata.reject_revision_record_mutation()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'metadata revision evidence is immutable' USING ERRCODE = '23514';
END;
$$;
COMMENT ON FUNCTION metadata.reject_revision_record_mutation() IS 'Rejects update, deletion and truncation of original revision attribution.';

CREATE TRIGGER tr_relationship_revision_records_immutable
BEFORE UPDATE OR DELETE ON metadata.relationship_revision_records
FOR EACH ROW EXECUTE FUNCTION metadata.reject_revision_record_mutation();
CREATE TRIGGER tr_relationship_revision_records_no_truncate
BEFORE TRUNCATE ON metadata.relationship_revision_records
FOR EACH STATEMENT EXECUTE FUNCTION metadata.reject_revision_record_mutation();

CREATE TRIGGER tr_signal_binding_revision_records_immutable
BEFORE UPDATE OR DELETE ON metadata.signal_binding_revision_records
FOR EACH ROW EXECUTE FUNCTION metadata.reject_revision_record_mutation();
CREATE TRIGGER tr_signal_binding_revision_records_no_truncate
BEFORE TRUNCATE ON metadata.signal_binding_revision_records
FOR EACH STATEMENT EXECUTE FUNCTION metadata.reject_revision_record_mutation();
