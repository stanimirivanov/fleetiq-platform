-- ============================================================================
-- Revisioned signal-binding persistence
-- ============================================================================
--
-- A signal binding attributes one decoded, protocol-neutral source signal to
-- one exact semantic property on a target asset. The source is the tuple
-- `(communication device, endpoint, signal ID)`; the target is the tuple
-- `(asset, property-definition version)`.
--
-- Bindings have two time dimensions. Recorded revision and recorded_at_ms
-- preserve what FleetIQ knew as commissioning data was corrected. The
-- half-open effective interval `[effective_start_ms, effective_end_ms)` records
-- when that attribution applied in the physical world. A NULL end is open.
--
-- Revisions use NUMERIC so every positive Rust u64 RecordRevision can be
-- represented. Time uses signed Unix milliseconds in BIGINT, matching the
-- domain contract without timestamp timezone or precision conversions.
--
-- Every revision is a complete immutable statement. The stable identity row's
-- mutable head serializes concurrent appends. Deferred checks require a history
-- that starts at one, is contiguous, ends at the head, and has non-decreasing
-- recorded time.
-- ============================================================================


-- ============================================================================
-- Stable binding identities
-- ============================================================================

-- One identity owns the append-only history for a logical attribution. Only
-- latest_revision changes; tenant_id and binding_id are permanent identity.
CREATE TABLE metadata.signal_bindings (
    tenant_id TEXT NOT NULL,
    binding_id TEXT NOT NULL,
    latest_revision NUMERIC NOT NULL,

    CONSTRAINT pk_signal_bindings
        PRIMARY KEY (tenant_id, binding_id),

    CONSTRAINT fk_signal_bindings_tenant
        FOREIGN KEY (tenant_id)
        REFERENCES metadata.tenants (tenant_id),

    CONSTRAINT ck_signal_bindings_id
        CHECK (metadata.is_valid_identifier(binding_id)),

    CONSTRAINT ck_signal_bindings_latest_revision
        CHECK (
            latest_revision = trunc(latest_revision)
            AND latest_revision BETWEEN 1 AND 18446744073709551615
        )
);


-- ============================================================================
-- Immutable binding revisions
-- ============================================================================

-- source_signal_id is an opaque stable identity emitted by a protocol package
-- or device profile. Its interpretation remains outside this metadata schema.
CREATE TABLE metadata.signal_binding_revisions (
    tenant_id TEXT NOT NULL,
    binding_id TEXT NOT NULL,
    revision NUMERIC NOT NULL,
    recorded_at_ms BIGINT NOT NULL,
    source_device_asset_id TEXT NOT NULL,
    source_endpoint_id TEXT NOT NULL,
    source_signal_id TEXT NOT NULL,
    target_asset_id TEXT NOT NULL,
    target_property_id TEXT NOT NULL,
    target_property_version BIGINT NOT NULL,
    effective_start_ms BIGINT NOT NULL,
    effective_end_ms BIGINT,

    CONSTRAINT pk_signal_binding_revisions
        PRIMARY KEY (tenant_id, binding_id, revision),

    CONSTRAINT fk_signal_binding_revisions_identity
        FOREIGN KEY (tenant_id, binding_id)
        REFERENCES metadata.signal_bindings (tenant_id, binding_id),

    -- This composite reference proves both that the device has communication
    -- capability and that the endpoint belongs to that device and tenant.
    CONSTRAINT fk_signal_binding_revisions_source_endpoint
        FOREIGN KEY (
            tenant_id,
            source_device_asset_id,
            source_endpoint_id
        )
        REFERENCES metadata.device_endpoints (
            tenant_id,
            device_asset_id,
            endpoint_id
        ),

    CONSTRAINT fk_signal_binding_revisions_target_asset
        FOREIGN KEY (tenant_id, target_asset_id)
        REFERENCES metadata.assets (tenant_id, asset_id),

    -- The property reference pins the semantic meaning used for attribution.
    CONSTRAINT fk_signal_binding_revisions_target_property
        FOREIGN KEY (
            tenant_id,
            target_property_id,
            target_property_version
        )
        REFERENCES metadata.property_definitions (
            tenant_id,
            property_id,
            version
        ),

    CONSTRAINT ck_signal_binding_revisions_revision
        CHECK (
            revision = trunc(revision)
            AND revision BETWEEN 1 AND 18446744073709551615
        ),

    CONSTRAINT ck_signal_binding_revisions_source_signal_id
        CHECK (metadata.is_valid_identifier(source_signal_id)),

    -- The exclusive end must follow the inclusive start. Adjacent bindings can
    -- meet at one instant without both applying at that instant.
    CONSTRAINT ck_signal_binding_revisions_effective_interval
        CHECK (
            effective_end_ms IS NULL
            OR effective_end_ms > effective_start_ms
        )
);


-- The reference is deferred because the repository creates the identity before
-- its first revision and advances the head in the same transaction as an append.
ALTER TABLE metadata.signal_bindings
ADD CONSTRAINT fk_signal_bindings_latest_revision
FOREIGN KEY (tenant_id, binding_id, latest_revision)
REFERENCES metadata.signal_binding_revisions (tenant_id, binding_id, revision)
DEFERRABLE INITIALLY DEFERRED;


-- ============================================================================
-- Semantic target compatibility
-- ============================================================================

-- A property definition can exist in a tenant without being supported by the
-- target asset's exact type version. This trigger follows the target asset to
-- asset_type_properties and rejects that semantically invalid attribution.
-- The check runs before insert, while the referenced asset and immutable type
-- definition are visible in the same transaction.
CREATE FUNCTION metadata.check_signal_binding_target_property()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM metadata.assets AS target_asset
        JOIN metadata.asset_type_properties AS supported_property
          ON supported_property.tenant_id = target_asset.tenant_id
         AND supported_property.type_id = target_asset.type_id
         AND supported_property.type_version = target_asset.type_version
        WHERE target_asset.tenant_id = NEW.tenant_id
          AND target_asset.asset_id = NEW.target_asset_id
          AND supported_property.property_id = NEW.target_property_id
          AND supported_property.property_version = NEW.target_property_version
    ) THEN
        RAISE EXCEPTION
            'Property %.% version % is not supported by target asset %.%',
            NEW.tenant_id,
            NEW.target_property_id,
            NEW.target_property_version,
            NEW.tenant_id,
            NEW.target_asset_id
            USING
                ERRCODE = '23503',
                CONSTRAINT = 'fk_signal_binding_target_property_support';
    END IF;

    RETURN NEW;
END;
$$;


CREATE TRIGGER signal_binding_revisions_check_target_property
BEFORE INSERT
ON metadata.signal_binding_revisions
FOR EACH ROW
EXECUTE FUNCTION metadata.check_signal_binding_target_property();


-- ============================================================================
-- Revision-history integrity
-- ============================================================================

-- Deferrable validation observes the transaction's final state so the identity
-- head and immutable revision can be written in either statement order.
CREATE FUNCTION metadata.check_signal_binding_history()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    checked_tenant_id TEXT;
    checked_binding_id TEXT;
    expected_latest NUMERIC;
    revision_count NUMERIC;
    minimum_revision NUMERIC;
    maximum_revision NUMERIC;
BEGIN
    checked_tenant_id := COALESCE(NEW.tenant_id, OLD.tenant_id);
    checked_binding_id := COALESCE(NEW.binding_id, OLD.binding_id);

    SELECT latest_revision
    INTO expected_latest
    FROM metadata.signal_bindings
    WHERE tenant_id = checked_tenant_id
      AND binding_id = checked_binding_id;

    IF NOT FOUND THEN
        RETURN NULL;
    END IF;

    SELECT count(*)::NUMERIC, min(revision), max(revision)
    INTO revision_count, minimum_revision, maximum_revision
    FROM metadata.signal_binding_revisions
    WHERE tenant_id = checked_tenant_id
      AND binding_id = checked_binding_id;

    IF minimum_revision <> 1
        OR maximum_revision <> expected_latest
        OR revision_count <> expected_latest
    THEN
        RAISE EXCEPTION
            'Signal binding %.% has a non-contiguous history ending at revision %',
            checked_tenant_id,
            checked_binding_id,
            expected_latest
            USING
                ERRCODE = '23514',
                CONSTRAINT = 'ck_signal_binding_revision_sequence';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM metadata.signal_binding_revisions AS current_revision
        JOIN metadata.signal_binding_revisions AS previous_revision
          ON previous_revision.tenant_id = current_revision.tenant_id
         AND previous_revision.binding_id = current_revision.binding_id
         AND previous_revision.revision = current_revision.revision - 1
        WHERE current_revision.tenant_id = checked_tenant_id
          AND current_revision.binding_id = checked_binding_id
          AND current_revision.recorded_at_ms < previous_revision.recorded_at_ms
    ) THEN
        RAISE EXCEPTION
            'Signal binding %.% has recorded time moving backwards between revisions',
            checked_tenant_id,
            checked_binding_id
            USING
                ERRCODE = '23514',
                CONSTRAINT = 'ck_signal_binding_recorded_time_order';
    END IF;

    RETURN NULL;
END;
$$;


CREATE CONSTRAINT TRIGGER signal_bindings_check_history
AFTER INSERT OR UPDATE
ON metadata.signal_bindings
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION metadata.check_signal_binding_history();


CREATE CONSTRAINT TRIGGER signal_binding_revisions_check_history
AFTER INSERT OR UPDATE OR DELETE
ON metadata.signal_binding_revisions
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION metadata.check_signal_binding_history();


-- ============================================================================
-- Immutability and stable identity enforcement
-- ============================================================================

CREATE FUNCTION metadata.prevent_signal_binding_revision_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION
        'Signal binding revisions are immutable; append a revision instead of using %',
        TG_OP
        USING ERRCODE = '55000';
END;
$$;


CREATE TRIGGER signal_binding_revisions_prevent_mutation
BEFORE UPDATE OR DELETE
ON metadata.signal_binding_revisions
FOR EACH ROW
EXECUTE FUNCTION metadata.prevent_signal_binding_revision_mutation();


-- The head may advance; the stable identity cannot be rewritten or deleted.
CREATE FUNCTION metadata.protect_signal_binding_identity()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION
            'Signal binding identities cannot be deleted'
            USING ERRCODE = '55000';
    END IF;

    IF NEW.tenant_id <> OLD.tenant_id OR NEW.binding_id <> OLD.binding_id THEN
        RAISE EXCEPTION
            'Signal binding identity fields are immutable'
            USING ERRCODE = '55000';
    END IF;

    RETURN NEW;
END;
$$;


CREATE TRIGGER signal_bindings_protect_identity
BEFORE UPDATE OR DELETE
ON metadata.signal_bindings
FOR EACH ROW
EXECUTE FUNCTION metadata.protect_signal_binding_identity();


-- ============================================================================
-- Bitemporal source lookup
-- ============================================================================

-- Source resolution first discovers identities that used this source by the
-- recorded-time cutoff, then loads each identity's latest known revision. The
-- final source filter is necessary because a correction may change the source.
CREATE INDEX ix_signal_binding_revisions_source_snapshot
ON metadata.signal_binding_revisions (
    tenant_id,
    source_device_asset_id,
    source_endpoint_id,
    source_signal_id,
    recorded_at_ms,
    binding_id
);


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON TABLE metadata.signal_bindings IS
    'Stable signal-binding identities and heads of their immutable revision histories.';

COMMENT ON COLUMN metadata.signal_bindings.latest_revision IS
    'Highest contiguous revision recorded for this binding identity.';

COMMENT ON TABLE metadata.signal_binding_revisions IS
    'Append-only complete revisions attributing decoded source signals to semantic asset properties.';

COMMENT ON COLUMN metadata.signal_binding_revisions.revision IS
    'Positive recorded revision represented by the complete Rust RecordRevision u64 range.';

COMMENT ON COLUMN metadata.signal_binding_revisions.recorded_at_ms IS
    'Signed Unix milliseconds when FleetIQ recorded this attribution revision.';

COMMENT ON COLUMN metadata.signal_binding_revisions.source_device_asset_id IS
    'Communication-device asset that produced the decoded signal.';

COMMENT ON COLUMN metadata.signal_binding_revisions.source_endpoint_id IS
    'Stable endpoint beneath the source communication device.';

COMMENT ON COLUMN metadata.signal_binding_revisions.source_signal_id IS
    'Protocol-neutral signal identity emitted by a decoder or device profile.';

COMMENT ON COLUMN metadata.signal_binding_revisions.target_asset_id IS
    'Asset whose semantic property receives observations from the source signal.';

COMMENT ON COLUMN metadata.signal_binding_revisions.target_property_id IS
    'Stable identity of the target semantic property definition.';

COMMENT ON COLUMN metadata.signal_binding_revisions.target_property_version IS
    'Exact target property version supported by the target asset type.';

COMMENT ON COLUMN metadata.signal_binding_revisions.effective_start_ms IS
    'Inclusive signed Unix-millisecond start of physical applicability.';

COMMENT ON COLUMN metadata.signal_binding_revisions.effective_end_ms IS
    'Exclusive signed Unix-millisecond end of physical applicability; NULL means open-ended.';

COMMENT ON FUNCTION metadata.check_signal_binding_target_property() IS
    'Rejects bindings whose exact target asset type does not support the selected property version.';

COMMENT ON FUNCTION metadata.check_signal_binding_history() IS
    'Deferrably enforces contiguous revisions, the current head, and recorded-time ordering.';

COMMENT ON FUNCTION metadata.prevent_signal_binding_revision_mutation() IS
    'Rejects UPDATE and DELETE operations against immutable signal-binding revisions.';

COMMENT ON FUNCTION metadata.protect_signal_binding_identity() IS
    'Allows head advancement while rejecting stable binding identity rewrites and deletion.';
