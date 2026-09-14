-- ============================================================================
-- Durable accepted-message log
-- ============================================================================
--
-- This migration introduces the append-only boundary for complete protocol
-- frames after device admission and before trusted observation assembly.
-- Every row preserves the tenant, communication device, immutable protocol-
-- package release, host receipt time, and exact opaque wire payload needed for
-- deterministic replay.
--
-- Message identity is scoped by tenant. The composite primary key supports
-- tenant-isolated idempotent append and gives later telemetry tables a safe
-- composite reference path. A repeated key is not sufficient to establish an
-- idempotent retry: the repository must compare every immutable column and
-- accept only an exact match.
--
-- Rows cannot be updated or deleted individually. Retention, archival, and
-- partition lifecycle require a later forward migration once their operational
-- guarantees are defined. Protocol framing already applies configured payload
-- limits before bytes reach this table; this schema does not freeze one global
-- maximum that would prevent package- or tenant-specific policies.
-- ============================================================================


-- ============================================================================
-- Ingestion namespace
-- ============================================================================

-- Durable ingestion facts are isolated from mutable configuration metadata so
-- access policy, retention, backup, and future partitioning can evolve around
-- their different operational lifecycle.
CREATE SCHEMA ingestion;


-- ============================================================================
-- Accepted protocol messages
-- ============================================================================

-- One row records the exact framed input after the configured durability
-- condition succeeds. The table contains no endpoint or signal identity because
-- one frame may decode into values from several device endpoints.
CREATE TABLE ingestion.accepted_messages (
    tenant_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    device_asset_id TEXT NOT NULL,
    protocol_package_id TEXT NOT NULL,
    protocol_package_version TEXT NOT NULL,

    -- Signed Unix milliseconds match the domain contract exactly and avoid a
    -- timezone or sub-millisecond conversion at the persistence boundary.
    received_at_ms BIGINT NOT NULL,

    -- The payload includes the complete protocol frame, including any headers
    -- and terminators selected by the package. Empty payloads remain valid
    -- because their meaning belongs to the custom protocol.
    payload BYTEA NOT NULL,

    CONSTRAINT pk_accepted_messages
        PRIMARY KEY (tenant_id, message_id),

    -- The composite reference proves both tenant ownership and that admission
    -- selected an asset with communication-device capability.
    CONSTRAINT fk_accepted_messages_device
        FOREIGN KEY (tenant_id, device_asset_id)
        REFERENCES metadata.communication_devices (tenant_id, device_asset_id),

    CONSTRAINT ck_accepted_messages_id
        CHECK (metadata.is_valid_identifier(message_id)),

    -- Package identity and release remain opaque validated identifiers until a
    -- package catalogue and activation model establish their own foreign key.
    CONSTRAINT ck_accepted_messages_package_id
        CHECK (metadata.is_valid_identifier(protocol_package_id)),

    CONSTRAINT ck_accepted_messages_package_version
        CHECK (metadata.is_valid_identifier(protocol_package_version))
);


-- ============================================================================
-- Append-only enforcement
-- ============================================================================

-- Accepted messages are immutable replay evidence. Corrections create new
-- messages or later semantic interpretations; they never rewrite wire input or
-- its admission and package provenance.
CREATE FUNCTION ingestion.prevent_accepted_message_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION
        'Accepted messages are append-only; % is not permitted',
        TG_OP
        USING ERRCODE = '55000';
END;
$$;


CREATE TRIGGER accepted_messages_prevent_mutation
BEFORE UPDATE OR DELETE
ON ingestion.accepted_messages
FOR EACH ROW
EXECUTE FUNCTION ingestion.prevent_accepted_message_mutation();


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON SCHEMA ingestion IS
    'Durable tenant-owned telemetry ingestion facts and replay evidence.';

COMMENT ON TABLE ingestion.accepted_messages IS
    'Append-only complete protocol frames retained after the configured durability condition succeeds.';

COMMENT ON COLUMN ingestion.accepted_messages.tenant_id IS
    'Tenant established by device admission and included in every identity and reference path.';

COMMENT ON COLUMN ingestion.accepted_messages.message_id IS
    'Stable message identity scoped by tenant and assigned before append for idempotent retry.';

COMMENT ON COLUMN ingestion.accepted_messages.device_asset_id IS
    'Admitted communication-device asset that supplied the frame.';

COMMENT ON COLUMN ingestion.accepted_messages.protocol_package_id IS
    'Stable identity of the protocol package used to frame and process the message.';

COMMENT ON COLUMN ingestion.accepted_messages.protocol_package_version IS
    'Immutable package release used for deterministic replay and decoded-observation identity.';

COMMENT ON COLUMN ingestion.accepted_messages.received_at_ms IS
    'Signed Unix milliseconds when the host received the complete framed message.';

COMMENT ON COLUMN ingestion.accepted_messages.payload IS
    'Exact opaque wire frame, including protocol headers and terminators; empty payloads are permitted.';

COMMENT ON FUNCTION ingestion.prevent_accepted_message_mutation() IS
    'Rejects row-level UPDATE and DELETE operations against durable accepted-message evidence.';
