-- ============================================================================
-- Attributed metadata registration
-- ============================================================================
-- Each record proves that a complete asset or device aggregate and its children
-- were committed by the registration use case. Older strict inserts deliberately
-- have no record: there is no trustworthy actor/time to backfill.
--
-- Additive, empty table and indexes; no existing-row rewrite or data backfill.
-- PostgreSQL briefly locks referenced tables while creating foreign keys.
-- Keep this migration on rollback: removing API routes does not erase evidence.

CREATE TABLE metadata.registration_records (
    tenant_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    -- Separate asset creation from attaching communication capability to it.
    registration_kind TEXT NOT NULL,
    -- A conditional stored reference enforces device existence only for device
    -- evidence; asset evidence has NULL here and uses the asset FK below.
    device_asset_id TEXT GENERATED ALWAYS AS (
        CASE WHEN registration_kind = 'device' THEN asset_id END
    ) STORED,
    actor_id TEXT NOT NULL,
    -- Exact signed Unix milliseconds, matching the domain/wire time contract.
    -- No database/client default: the host supplies the first receipt instant.
    recorded_at_ms BIGINT NOT NULL,

    CONSTRAINT pk_registration_records
        PRIMARY KEY (tenant_id, asset_id, registration_kind),
    CONSTRAINT fk_registration_records_asset
        FOREIGN KEY (tenant_id, asset_id)
        REFERENCES metadata.assets (tenant_id, asset_id) ON DELETE RESTRICT,
    CONSTRAINT fk_registration_records_device
        FOREIGN KEY (tenant_id, device_asset_id)
        REFERENCES metadata.communication_devices (tenant_id, device_asset_id)
        ON DELETE RESTRICT,
    CONSTRAINT fk_registration_records_actor
        FOREIGN KEY (actor_id)
        REFERENCES identity.actors (actor_id) ON DELETE RESTRICT,
    CONSTRAINT ck_registration_records_kind
        CHECK (registration_kind IN ('asset', 'device'))
);

-- Supports actor-reference maintenance without scanning all registration records.
CREATE INDEX ix_registration_records_actor
ON metadata.registration_records (actor_id);

-- Evidence is write-once. Reject updates and deletes, including SQL issued outside
-- the application; no retry may overwrite the original actor or receipt time.
CREATE FUNCTION metadata.reject_registration_record_mutation()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'metadata registration evidence is immutable'
        USING ERRCODE = '23514';
END;
$$;
CREATE TRIGGER tr_registration_records_immutable
BEFORE UPDATE OR DELETE ON metadata.registration_records
FOR EACH ROW EXECUTE FUNCTION metadata.reject_registration_record_mutation();
CREATE TRIGGER tr_registration_records_no_truncate
BEFORE TRUNCATE ON metadata.registration_records
FOR EACH STATEMENT EXECUTE FUNCTION metadata.reject_registration_record_mutation();

COMMENT ON TABLE metadata.registration_records IS
    'Write-once evidence committed atomically with an attributed asset or device registration; absence means attribution is unknown.';
COMMENT ON COLUMN metadata.registration_records.tenant_id IS
    'Owning tenant; part of every registration identity and asset reference.';
COMMENT ON COLUMN metadata.registration_records.asset_id IS
    'Internal asset identity, also reused by communication capability; never an IMEI or serial number.';
COMMENT ON COLUMN metadata.registration_records.registration_kind IS
    'Closed creation operation: asset or device; both can have independent evidence for one asset.';
COMMENT ON COLUMN metadata.registration_records.device_asset_id IS
    'Computed device-only asset reference; prevents device evidence without communication capability.';
COMMENT ON COLUMN metadata.registration_records.actor_id IS
    'Original authenticated actor carrying exact tenant metadata.manage authority, not the retrying actor.';
COMMENT ON COLUMN metadata.registration_records.recorded_at_ms IS
    'Original host receipt instant in signed Unix milliseconds; retained on retries, never backfilled for legacy objects.';
COMMENT ON FUNCTION metadata.reject_registration_record_mutation() IS
    'Rejects modification, deletion and truncation of attributed registration evidence.';
