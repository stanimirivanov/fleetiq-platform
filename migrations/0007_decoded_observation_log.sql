-- ============================================================================
-- Atomic decoded-message result persistence
-- ============================================================================
--
-- This migration introduces the durable boundary after protocol decoding and
-- before semantic signal binding. A result marker records that one accepted
-- message was decoded by its pinned protocol-package release. Ordered child
-- rows preserve every typed observation emitted by that decode.
--
-- The marker exists even when observation_count is zero. Consumers can
-- therefore distinguish successful decoding with no telemetry from unfinished
-- or failed work without inferring state from missing child rows.
--
-- Result and observation rows are append-only. The application adapter must
-- insert one marker and its complete child collection in one transaction,
-- compare all immutable content on conflict, and expose no partial success.
-- Database keys prevent duplicate positions and cross-tenant or cross-package
-- references; application batch validation supplies the complete-collection
-- invariant without an aggregate constraint trigger on every inserted row.
--
-- Adding the accepted-message unique constraint builds one redundant unique
-- index and takes PostgreSQL's normal ALTER TABLE lock while it is installed.
-- The existing primary key already proves that the expanded tuple is unique,
-- so no data backfill or conflict remediation is required. On a high-volume
-- deployed table, schedule this migration within the schema-change window.
-- ============================================================================


-- ============================================================================
-- Exact accepted-message decoder reference
-- ============================================================================

-- PostgreSQL requires referenced columns to be covered by a unique constraint.
-- Including the package fields lets the result marker prove that it names the
-- exact immutable decoder release stored with the accepted raw message.
ALTER TABLE ingestion.accepted_messages
    ADD CONSTRAINT uq_accepted_messages_decoder_reference
    UNIQUE (
        tenant_id,
        message_id,
        protocol_package_id,
        protocol_package_version
    );


-- ============================================================================
-- Decoded message results
-- ============================================================================

-- One row is durable completion evidence for decoding one accepted message.
-- observation_count participates in exact-retry comparison and remains zero
-- for a valid decode that emits no telemetry.
CREATE TABLE ingestion.decoded_message_results (
    tenant_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    protocol_package_id TEXT NOT NULL,
    protocol_package_version TEXT NOT NULL,
    observation_count BIGINT NOT NULL,

    CONSTRAINT pk_decoded_message_results
        PRIMARY KEY (
            tenant_id,
            message_id,
            protocol_package_id,
            protocol_package_version
        ),

    -- The expanded composite reference enforces tenant ownership and prevents
    -- a result from claiming a package release other than the one used when
    -- the raw message was accepted.
    CONSTRAINT fk_decoded_message_results_message
        FOREIGN KEY (
            tenant_id,
            message_id,
            protocol_package_id,
            protocol_package_version
        )
        REFERENCES ingestion.accepted_messages (
            tenant_id,
            message_id,
            protocol_package_id,
            protocol_package_version
        )
        ON DELETE NO ACTION,

    -- Output identity uses u32 positions. A complete sequence may contain
    -- positions 0 through 2^32 - 1, hence at most 2^32 observations.
    CONSTRAINT ck_decoded_message_results_observation_count
        CHECK (observation_count BETWEEN 0 AND 4294967296)
);


-- ============================================================================
-- Ordered decoded observations
-- ============================================================================

-- Each row stores decoder-owned source and measurement fields. Tenant,
-- admitted communication-device identity, host receipt time, accepted-message
-- identity, and package provenance are obtained through the parent result and
-- accepted message rather than duplicated as independently mutable values.
CREATE TABLE ingestion.decoded_observations (
    tenant_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    protocol_package_id TEXT NOT NULL,
    protocol_package_version TEXT NOT NULL,

    -- The zero-based package emission position is part of deterministic replay
    -- identity and preserves order without relying on insertion order.
    output_index BIGINT NOT NULL,

    endpoint_id TEXT NOT NULL,
    signal_id TEXT NOT NULL,

    -- NULL means the device supplied no timestamp. It never falls back to the
    -- host receipt time implicitly; later event-time policy makes that choice.
    device_recorded_at_ms BIGINT,

    value_kind TEXT NOT NULL,
    value_boolean BOOLEAN,
    value_integer BIGINT,
    value_decimal NUMERIC,
    value_text TEXT,

    -- NULL represents a unitless numeric value or any non-numeric value. Units
    -- are forbidden for Boolean and text values by a separate check.
    source_unit TEXT,
    quality TEXT NOT NULL,

    CONSTRAINT pk_decoded_observations
        PRIMARY KEY (
            tenant_id,
            message_id,
            protocol_package_id,
            protocol_package_version,
            output_index
        ),

    CONSTRAINT fk_decoded_observations_result
        FOREIGN KEY (
            tenant_id,
            message_id,
            protocol_package_id,
            protocol_package_version
        )
        REFERENCES ingestion.decoded_message_results (
            tenant_id,
            message_id,
            protocol_package_id,
            protocol_package_version
        )
        ON DELETE NO ACTION,

    CONSTRAINT ck_decoded_observations_output_index
        CHECK (output_index BETWEEN 0 AND 4294967295),

    CONSTRAINT ck_decoded_observations_endpoint_id
        CHECK (metadata.is_valid_identifier(endpoint_id)),

    CONSTRAINT ck_decoded_observations_signal_id
        CHECK (metadata.is_valid_identifier(signal_id)),

    CONSTRAINT ck_decoded_observations_value_kind
        CHECK (value_kind IN ('boolean', 'integer', 'decimal', 'text')),

    -- Exactly one typed storage column must be populated, and it must agree
    -- with value_kind. This prevents ambiguous or lossy reconstruction.
    CONSTRAINT ck_decoded_observations_typed_value
        CHECK (
            (value_kind = 'boolean'
                AND value_boolean IS NOT NULL
                AND value_integer IS NULL
                AND value_decimal IS NULL
                AND value_text IS NULL)
            OR (value_kind = 'integer'
                AND value_boolean IS NULL
                AND value_integer IS NOT NULL
                AND value_decimal IS NULL
                AND value_text IS NULL)
            OR (value_kind = 'decimal'
                AND value_boolean IS NULL
                AND value_integer IS NULL
                AND value_decimal IS NOT NULL
                AND value_text IS NULL)
            OR (value_kind = 'text'
                AND value_boolean IS NULL
                AND value_integer IS NULL
                AND value_decimal IS NULL
                AND value_text IS NOT NULL)
        ),

    CONSTRAINT ck_decoded_observations_unit
        CHECK (source_unit IS NULL OR value_kind IN ('integer', 'decimal')),

    CONSTRAINT ck_decoded_observations_unit_id
        CHECK (
            source_unit IS NULL
            OR metadata.is_valid_identifier(source_unit)
        ),

    CONSTRAINT ck_decoded_observations_quality
        CHECK (quality IN ('good', 'uncertain', 'invalid'))
);


-- ============================================================================
-- Append-only enforcement
-- ============================================================================

-- Decoded results are immutable derivation evidence. Reprocessing creates a
-- separately identified result rather than rewriting an existing batch.
CREATE FUNCTION ingestion.prevent_decoded_result_mutation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION
        'Decoded ingestion facts in %.% are append-only; % is not permitted',
        TG_TABLE_SCHEMA,
        TG_TABLE_NAME,
        TG_OP
        USING ERRCODE = '55000';
END;
$$;


CREATE TRIGGER decoded_message_results_prevent_mutation
BEFORE UPDATE OR DELETE
ON ingestion.decoded_message_results
FOR EACH ROW
EXECUTE FUNCTION ingestion.prevent_decoded_result_mutation();


CREATE TRIGGER decoded_observations_prevent_mutation
BEFORE UPDATE OR DELETE
ON ingestion.decoded_observations
FOR EACH ROW
EXECUTE FUNCTION ingestion.prevent_decoded_result_mutation();


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON CONSTRAINT uq_accepted_messages_decoder_reference
ON ingestion.accepted_messages IS
    'Unique reference path proving the exact protocol-package release pinned to an accepted message.';

COMMENT ON TABLE ingestion.decoded_message_results IS
    'Append-only completion markers for atomic, idempotent decoded-message results, including zero-output decoding.';

COMMENT ON COLUMN ingestion.decoded_message_results.observation_count IS
    'Complete package output count used to represent empty decoding and compare idempotent retries.';

COMMENT ON TABLE ingestion.decoded_observations IS
    'Append-only ordered protocol outputs retained before semantic signal binding.';

COMMENT ON COLUMN ingestion.decoded_observations.output_index IS
    'Zero-based package emission position and deterministic identity within one decoded-message result.';

COMMENT ON COLUMN ingestion.decoded_observations.endpoint_id IS
    'Protocol-neutral endpoint identity beneath the admitted communication device.';

COMMENT ON COLUMN ingestion.decoded_observations.signal_id IS
    'Stable source-signal identity emitted by the protocol package or device profile.';

COMMENT ON COLUMN ingestion.decoded_observations.device_recorded_at_ms IS
    'Optional signed Unix milliseconds reported by the device; NULL means absent rather than inferred.';

COMMENT ON COLUMN ingestion.decoded_observations.value_kind IS
    'Discriminator selecting exactly one typed decoded-value column.';

COMMENT ON COLUMN ingestion.decoded_observations.value_decimal IS
    'Exact PostgreSQL NUMERIC representation of an arbitrary-precision decoded decimal.';

COMMENT ON COLUMN ingestion.decoded_observations.source_unit IS
    'Optional decoder-declared unit for integer or decimal source values before semantic conversion.';

COMMENT ON COLUMN ingestion.decoded_observations.quality IS
    'Closed source-reported quality: good, uncertain, or invalid.';

COMMENT ON FUNCTION ingestion.prevent_decoded_result_mutation() IS
    'Rejects row-level UPDATE and DELETE operations against decoded-result markers and observations.';
