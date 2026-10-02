-- ============================================================================
-- Immutable semantic property history
-- ============================================================================
--
-- One row records that an already durable decoded observation was interpreted
-- through one exact binding revision using an explicit event-time basis. The
-- row keeps interpretation identity and context; the immutable observation and
-- binding rows remain the authoritative value, quality, source, and target.
-- This avoids a second independently mutable copy of telemetry values.
--
-- The table is tenant-owned and append-only. Composite foreign keys preserve
-- exact decoder and binding provenance; a trigger checks the cross-table
-- semantic and temporal conditions that foreign keys cannot express. There is
-- no backfill: existing decoded facts have not yet been processed by a worker.
-- This additive migration takes only the ordinary short schema locks of an
-- empty new table; it does not rewrite ingestion or metadata tables.
-- ============================================================================

CREATE SCHEMA history;

-- The primary key is the deterministic AssetPropertyChangeId. Knowledge and
-- effective cutoffs are first-append evidence, not identity: reprocessing the
-- same observation against the same binding revision at a later cutoff may
-- legitimately produce the same semantic interpretation.
CREATE TABLE history.asset_property_changes (
    tenant_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    protocol_package_id TEXT NOT NULL,
    protocol_package_version TEXT NOT NULL,
    output_index BIGINT NOT NULL,
    binding_id TEXT NOT NULL,
    binding_revision NUMERIC NOT NULL,
    event_time_basis TEXT NOT NULL,
    event_at_ms BIGINT NOT NULL,
    resolution_effective_at_ms BIGINT NOT NULL,
    resolution_known_at_ms BIGINT NOT NULL,

    CONSTRAINT pk_asset_property_changes PRIMARY KEY (
        tenant_id, message_id, protocol_package_id,
        protocol_package_version, output_index, binding_id,
        binding_revision, event_time_basis
    ),
    CONSTRAINT fk_asset_property_changes_observation FOREIGN KEY (
        tenant_id, message_id, protocol_package_id,
        protocol_package_version, output_index
    ) REFERENCES ingestion.decoded_observations (
        tenant_id, message_id, protocol_package_id,
        protocol_package_version, output_index
    ) ON DELETE NO ACTION,
    CONSTRAINT fk_asset_property_changes_binding FOREIGN KEY (
        tenant_id, binding_id, binding_revision
    ) REFERENCES metadata.signal_binding_revisions (
        tenant_id, binding_id, revision
    ) ON DELETE NO ACTION,
    CONSTRAINT ck_asset_property_changes_output_index
        CHECK (output_index BETWEEN 0 AND 4294967295),
    CONSTRAINT ck_asset_property_changes_binding_revision
        CHECK (binding_revision = trunc(binding_revision)
            AND binding_revision BETWEEN 1 AND 18446744073709551615),
    CONSTRAINT ck_asset_property_changes_event_time_basis
        CHECK (event_time_basis IN ('device_recorded', 'received_at'))
);

-- A row must describe a source observation and binding that agree on device,
-- endpoint, signal, property kind and canonical unit. The chosen binding must
-- be the latest revision of its identity known at the recorded cutoff and
-- effective at the supplied physical-time cutoff. Event time must come from
-- the selected immutable timestamp, never from an independently invented one.
CREATE FUNCTION history.check_asset_property_change_lineage()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM ingestion.decoded_observations AS observation
        JOIN ingestion.accepted_messages AS message
          USING (tenant_id, message_id, protocol_package_id,
                 protocol_package_version)
        JOIN metadata.signal_binding_revisions AS binding
          ON binding.tenant_id = observation.tenant_id
         AND binding.binding_id = NEW.binding_id
         AND binding.revision = NEW.binding_revision
        JOIN metadata.property_definitions AS property
          ON property.tenant_id = binding.tenant_id
         AND property.property_id = binding.target_property_id
         AND property.version = binding.target_property_version
        WHERE observation.tenant_id = NEW.tenant_id
          AND observation.message_id = NEW.message_id
          AND observation.protocol_package_id = NEW.protocol_package_id
          AND observation.protocol_package_version = NEW.protocol_package_version
          AND observation.output_index = NEW.output_index
          AND message.device_asset_id = binding.source_device_asset_id
          AND observation.endpoint_id = binding.source_endpoint_id
          AND observation.signal_id = binding.source_signal_id
          AND property.value_kind = observation.value_kind
          AND property.canonical_unit IS NOT DISTINCT FROM observation.source_unit
          AND binding.recorded_at_ms <= NEW.resolution_known_at_ms
          AND binding.effective_start_ms <= NEW.resolution_effective_at_ms
          AND (binding.effective_end_ms IS NULL
               OR NEW.resolution_effective_at_ms < binding.effective_end_ms)
          AND NOT EXISTS (
              SELECT 1
              FROM metadata.signal_binding_revisions AS later
              WHERE later.tenant_id = binding.tenant_id
                AND later.binding_id = binding.binding_id
                AND later.revision > binding.revision
                AND later.recorded_at_ms <= NEW.resolution_known_at_ms
          )
          AND (
              (NEW.event_time_basis = 'device_recorded'
               AND observation.device_recorded_at_ms = NEW.event_at_ms)
              OR (NEW.event_time_basis = 'received_at'
                  AND message.received_at_ms = NEW.event_at_ms)
          )
    ) THEN
        RAISE EXCEPTION 'Asset property change lineage is inconsistent'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'ck_asset_property_change_lineage';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER asset_property_changes_check_lineage
BEFORE INSERT ON history.asset_property_changes
FOR EACH ROW
EXECUTE FUNCTION history.check_asset_property_change_lineage();

-- Corrections create new binding-revision identities rather than rewriting a
-- previously recorded semantic interpretation. Retention must later use an
-- explicit archival and partition policy, not row-level deletion.
CREATE FUNCTION history.prevent_property_change_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Semantic property history is append-only; % is not permitted', TG_OP
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER asset_property_changes_prevent_mutation
BEFORE UPDATE OR DELETE ON history.asset_property_changes
FOR EACH ROW
EXECUTE FUNCTION history.prevent_property_change_mutation();

COMMENT ON SCHEMA history IS
    'Append-only semantic interpretations derived from durable ingestion and immutable metadata.';
COMMENT ON TABLE history.asset_property_changes IS
    'One immutable asset-property change identity with event-time and first-append resolution context.';
COMMENT ON COLUMN history.asset_property_changes.event_time_basis IS
    'Explicit device_recorded or received_at selection; never an implicit timestamp fallback.';
COMMENT ON COLUMN history.asset_property_changes.event_at_ms IS
    'Selected signed Unix-millisecond event time, checked against the immutable source fact.';
COMMENT ON COLUMN history.asset_property_changes.resolution_effective_at_ms IS
    'Signed Unix-millisecond physical-time cutoff used when the binding was selected.';
COMMENT ON COLUMN history.asset_property_changes.resolution_known_at_ms IS
    'Signed Unix-millisecond recorded-time cutoff used when the binding was selected.';
COMMENT ON FUNCTION history.check_asset_property_change_lineage() IS
    'Rejects semantic history that disagrees with durable source, binding, definition, or temporal facts.';
COMMENT ON FUNCTION history.prevent_property_change_mutation() IS
    'Rejects row-level UPDATE and DELETE against immutable semantic property history.';
