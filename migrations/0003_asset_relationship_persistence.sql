-- ============================================================================
-- Revisioned asset relationship persistence
-- ============================================================================
--
-- This migration persists FleetIQ's directed, typed asset graph with two time
-- dimensions:
--
--   1. Recorded time and revision
--
--      A stable relationship identity owns an append-only sequence of complete
--      revisions. The sequence records when FleetIQ learned or corrected a
--      relationship fact. The mutable identity row points to the latest
--      recorded revision and is used to serialize concurrent appends.
--
--   2. Effective time
--
--      Every revision contains a half-open interval `[start, end)` describing
--      when that fact applied in the physical world. NULL effective_end_ms means
--      the fact remains effective indefinitely.
--
-- Recorded and effective instants use signed Unix milliseconds in BIGINT. This
-- intentionally matches the domain's exact i64 contract and avoids timestamp
-- precision, timezone, and range conversions at the persistence boundary.
--
-- RecordRevision uses the complete non-zero Rust u64 range. PostgreSQL NUMERIC
-- preserves that range; checks require integral values between 1 and
-- 18446744073709551615.
--
-- Revisions are immutable. Corrections append another revision under the same
-- relationship identity. Deferred constraints verify at commit that revision
-- history is contiguous, that the head points to the final revision, and that
-- recorded time does not move backwards as revision numbers increase.
-- ============================================================================


-- ============================================================================
-- Stable relationship identities
-- ============================================================================

-- One row represents the stable identity shared by every recorded revision of a
-- relationship. Only latest_revision is mutable; the tenant and relationship ID
-- are permanent identity fields.
CREATE TABLE metadata.asset_relationships (
    tenant_id TEXT NOT NULL,
    relationship_id TEXT NOT NULL,
    latest_revision NUMERIC NOT NULL,

    CONSTRAINT pk_asset_relationships
        PRIMARY KEY (tenant_id, relationship_id),

    CONSTRAINT fk_asset_relationships_tenant
        FOREIGN KEY (tenant_id)
        REFERENCES metadata.tenants (tenant_id),

    CONSTRAINT ck_asset_relationships_id
        CHECK (metadata.is_valid_identifier(relationship_id)),

    CONSTRAINT ck_asset_relationships_latest_revision
        CHECK (
            latest_revision = trunc(latest_revision)
            AND latest_revision BETWEEN 1 AND 18446744073709551615
        )
);


-- ============================================================================
-- Immutable relationship revisions
-- ============================================================================

-- Each row is a complete immutable statement about one relationship identity.
-- Endpoint roles are ordered and derive their meaning from the referenced exact
-- relationship-type definition version.
CREATE TABLE metadata.asset_relationship_revisions (
    tenant_id TEXT NOT NULL,
    relationship_id TEXT NOT NULL,
    revision NUMERIC NOT NULL,
    recorded_at_ms BIGINT NOT NULL,
    relationship_type_id TEXT NOT NULL,
    relationship_type_version BIGINT NOT NULL,
    source_asset_id TEXT NOT NULL,
    target_asset_id TEXT NOT NULL,
    effective_start_ms BIGINT NOT NULL,
    effective_end_ms BIGINT,

    CONSTRAINT pk_asset_relationship_revisions
        PRIMARY KEY (tenant_id, relationship_id, revision),

    CONSTRAINT fk_asset_relationship_revisions_identity
        FOREIGN KEY (tenant_id, relationship_id)
        REFERENCES metadata.asset_relationships (tenant_id, relationship_id),

    -- The composite key prevents a revision from using a relationship type
    -- owned by another tenant or a different semantic version.
    CONSTRAINT fk_asset_relationship_revisions_type
        FOREIGN KEY (
            tenant_id,
            relationship_type_id,
            relationship_type_version
        )
        REFERENCES metadata.relationship_type_definitions (
            tenant_id,
            relationship_type_id,
            version
        ),

    -- Both endpoint references include tenant_id, making cross-tenant graph
    -- edges structurally impossible.
    CONSTRAINT fk_asset_relationship_revisions_source
        FOREIGN KEY (tenant_id, source_asset_id)
        REFERENCES metadata.assets (tenant_id, asset_id),

    CONSTRAINT fk_asset_relationship_revisions_target
        FOREIGN KEY (tenant_id, target_asset_id)
        REFERENCES metadata.assets (tenant_id, asset_id),

    CONSTRAINT ck_asset_relationship_revisions_revision
        CHECK (
            revision = trunc(revision)
            AND revision BETWEEN 1 AND 18446744073709551615
        ),

    -- The exclusive end must be later than the inclusive start. Adjacent
    -- intervals can therefore meet at one instant without overlapping.
    CONSTRAINT ck_asset_relationship_revisions_effective_interval
        CHECK (
            effective_end_ms IS NULL
            OR effective_end_ms > effective_start_ms
        )
);


-- The head reference is deferred because the repository creates a new identity
-- before inserting its first revision and advances an existing identity in the
-- same transaction as the new revision.
ALTER TABLE metadata.asset_relationships
ADD CONSTRAINT fk_asset_relationships_latest_revision
FOREIGN KEY (tenant_id, relationship_id, latest_revision)
REFERENCES metadata.asset_relationship_revisions (
    tenant_id,
    relationship_id,
    revision
)
DEFERRABLE INITIALLY DEFERRED;


-- ============================================================================
-- Revision-history integrity
-- ============================================================================

-- Verifies the complete history for one relationship at transaction commit.
-- A valid history starts at one, has no gaps, ends at latest_revision, and has
-- non-decreasing recorded timestamps.
CREATE FUNCTION metadata.check_asset_relationship_history()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    checked_tenant_id TEXT;
    checked_relationship_id TEXT;
    expected_latest NUMERIC;
    revision_count NUMERIC;
    minimum_revision NUMERIC;
    maximum_revision NUMERIC;
BEGIN
    checked_tenant_id := COALESCE(NEW.tenant_id, OLD.tenant_id);
    checked_relationship_id := COALESCE(
        NEW.relationship_id,
        OLD.relationship_id
    );

    SELECT latest_revision
    INTO expected_latest
    FROM metadata.asset_relationships
    WHERE tenant_id = checked_tenant_id
      AND relationship_id = checked_relationship_id;

    -- Identity deletion is rejected by a separate trigger. This branch allows
    -- PostgreSQL to evaluate trigger queues safely during migration operations.
    IF NOT FOUND THEN
        RETURN NULL;
    END IF;

    SELECT count(*)::NUMERIC, min(revision), max(revision)
    INTO revision_count, minimum_revision, maximum_revision
    FROM metadata.asset_relationship_revisions
    WHERE tenant_id = checked_tenant_id
      AND relationship_id = checked_relationship_id;

    IF minimum_revision <> 1
        OR maximum_revision <> expected_latest
        OR revision_count <> expected_latest
    THEN
        RAISE EXCEPTION
            'Relationship %.% has a non-contiguous history ending at revision %',
            checked_tenant_id,
            checked_relationship_id,
            expected_latest
            USING
                ERRCODE = '23514',
                CONSTRAINT = 'ck_asset_relationship_revision_sequence';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM metadata.asset_relationship_revisions AS current_revision
        JOIN metadata.asset_relationship_revisions AS previous_revision
          ON previous_revision.tenant_id = current_revision.tenant_id
         AND previous_revision.relationship_id = current_revision.relationship_id
         AND previous_revision.revision = current_revision.revision - 1
        WHERE current_revision.tenant_id = checked_tenant_id
          AND current_revision.relationship_id = checked_relationship_id
          AND current_revision.recorded_at_ms < previous_revision.recorded_at_ms
    ) THEN
        RAISE EXCEPTION
            'Relationship %.% has recorded time moving backwards between revisions',
            checked_tenant_id,
            checked_relationship_id
            USING
                ERRCODE = '23514',
                CONSTRAINT = 'ck_asset_relationship_recorded_time_order';
    END IF;

    RETURN NULL;
END;
$$;


CREATE CONSTRAINT TRIGGER asset_relationships_check_history
AFTER INSERT OR UPDATE
ON metadata.asset_relationships
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION metadata.check_asset_relationship_history();


CREATE CONSTRAINT TRIGGER asset_relationship_revisions_check_history
AFTER INSERT OR UPDATE OR DELETE
ON metadata.asset_relationship_revisions
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION metadata.check_asset_relationship_history();


-- ============================================================================
-- Immutability and stable identity enforcement
-- ============================================================================

-- Recorded revisions are historical facts. A correction must append a new
-- revision instead of rewriting or removing an earlier interpretation.
CREATE FUNCTION metadata.prevent_relationship_revision_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION
        'Asset relationship revisions are immutable; append a new revision instead of using %',
        TG_OP
        USING ERRCODE = '55000';
END;
$$;


CREATE TRIGGER asset_relationship_revisions_prevent_mutation
BEFORE UPDATE OR DELETE
ON metadata.asset_relationship_revisions
FOR EACH ROW
EXECUTE FUNCTION metadata.prevent_relationship_revision_mutation();


-- Allows the relationship head to advance while preventing identity rewrites or
-- deletion of the stable relationship record.
CREATE FUNCTION metadata.protect_asset_relationship_identity()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION
            'Asset relationship identities cannot be deleted'
            USING ERRCODE = '55000';
    END IF;

    IF NEW.tenant_id <> OLD.tenant_id
        OR NEW.relationship_id <> OLD.relationship_id
    THEN
        RAISE EXCEPTION
            'Asset relationship identity fields are immutable'
            USING ERRCODE = '55000';
    END IF;

    RETURN NEW;
END;
$$;


CREATE TRIGGER asset_relationships_protect_identity
BEFORE UPDATE OR DELETE
ON metadata.asset_relationships
FOR EACH ROW
EXECUTE FUNCTION metadata.protect_asset_relationship_identity();


-- ============================================================================
-- Bitemporal lookup indexes
-- ============================================================================

-- Candidate discovery for relationship snapshots begins from either endpoint.
-- recorded_at_ms limits facts to those known by the requested recorded-time
-- cutoff; relationship_id then supports deduplication before revision lookup.
CREATE INDEX ix_asset_relationship_revisions_source_snapshot
ON metadata.asset_relationship_revisions (
    tenant_id,
    source_asset_id,
    recorded_at_ms,
    relationship_id
);


CREATE INDEX ix_asset_relationship_revisions_target_snapshot
ON metadata.asset_relationship_revisions (
    tenant_id,
    target_asset_id,
    recorded_at_ms,
    relationship_id
);


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON TABLE metadata.asset_relationships IS
    'Stable relationship identities and mutable pointers to their latest immutable revision.';

COMMENT ON COLUMN metadata.asset_relationships.latest_revision IS
    'Highest contiguous revision recorded for this relationship identity.';

COMMENT ON TABLE metadata.asset_relationship_revisions IS
    'Append-only complete revisions of directed, typed, effective-dated asset relationships.';

COMMENT ON COLUMN metadata.asset_relationship_revisions.revision IS
    'Positive recorded revision represented by the full Rust RecordRevision u64 range.';

COMMENT ON COLUMN metadata.asset_relationship_revisions.recorded_at_ms IS
    'Signed Unix milliseconds when FleetIQ recorded this revision.';

COMMENT ON COLUMN metadata.asset_relationship_revisions.source_asset_id IS
    'Asset occupying the relationship type''s directed source role.';

COMMENT ON COLUMN metadata.asset_relationship_revisions.target_asset_id IS
    'Asset occupying the relationship type''s directed target role.';

COMMENT ON COLUMN metadata.asset_relationship_revisions.effective_start_ms IS
    'Inclusive signed Unix-millisecond start of physical applicability.';

COMMENT ON COLUMN metadata.asset_relationship_revisions.effective_end_ms IS
    'Exclusive signed Unix-millisecond end of physical applicability; NULL means open-ended.';

COMMENT ON FUNCTION metadata.check_asset_relationship_history() IS
    'Deferrably enforces contiguous revisions, the current head, and recorded-time ordering.';

COMMENT ON FUNCTION metadata.prevent_relationship_revision_mutation() IS
    'Rejects UPDATE and DELETE operations against immutable relationship revisions.';

COMMENT ON FUNCTION metadata.protect_asset_relationship_identity() IS
    'Allows head advancement while rejecting stable identity rewrites and deletion.';
