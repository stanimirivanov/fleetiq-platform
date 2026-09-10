-- ============================================================================
-- Communication device and endpoint persistence
-- ============================================================================
--
-- This migration marks an existing FleetIQ asset as a communication device and
-- records the stable physical or logical sources exposed beneath it.
--
-- Device identity deliberately reuses asset identity. A tracker, gateway, or
-- controller remains an asset with a semantic type, operator-facing name, and
-- typed external identifiers; the communication-device row adds capability
-- without creating a competing identity.
--
-- Endpoints locate observations within a device. Examples can include the
-- device's onboard measurements, a CAN interface, a serial channel, or an
-- identified peripheral. Endpoint kinds are validated identifiers rather than
-- a closed SQL enum so future device profiles can extend the vocabulary.
--
-- This schema does not assign vendor field selectors, protocol packages, or
-- target asset properties. Those concerns belong to device profiles, protocol
-- activation, and effective-dated signal bindings respectively.
-- ============================================================================


-- ============================================================================
-- Communication capability attached to assets
-- ============================================================================

-- One row designates an existing asset as a communication device. The asset's
-- tenant and asset_id remain the only physical identity; this table introduces
-- no device-specific surrogate key.
CREATE TABLE metadata.communication_devices (
    tenant_id TEXT NOT NULL,
    device_asset_id TEXT NOT NULL,

    CONSTRAINT pk_communication_devices
        PRIMARY KEY (tenant_id, device_asset_id),

    -- Including tenant_id structurally prevents communication capability from
    -- being attached to an asset owned by another tenant.
    CONSTRAINT fk_communication_devices_asset
        FOREIGN KEY (tenant_id, device_asset_id)
        REFERENCES metadata.assets (tenant_id, asset_id)
);


-- ============================================================================
-- Device endpoints
-- ============================================================================

-- An endpoint is a stable source identity scoped by one communication device.
-- The same endpoint_id may appear beneath different devices because source
-- identity used by later bindings is the device/endpoint pair.
CREATE TABLE metadata.device_endpoints (
    tenant_id TEXT NOT NULL,
    device_asset_id TEXT NOT NULL,
    endpoint_id TEXT NOT NULL,
    endpoint_kind_id TEXT NOT NULL,
    display_name TEXT NOT NULL,

    -- Declaration order gives commissioning and diagnostics screens a stable
    -- presentation order without treating the display name as identity.
    endpoint_ordinal BIGINT NOT NULL,

    CONSTRAINT pk_device_endpoints
        PRIMARY KEY (tenant_id, device_asset_id, endpoint_id),

    CONSTRAINT fk_device_endpoints_device
        FOREIGN KEY (tenant_id, device_asset_id)
        REFERENCES metadata.communication_devices (tenant_id, device_asset_id),

    CONSTRAINT ck_device_endpoints_id
        CHECK (metadata.is_valid_identifier(endpoint_id)),

    CONSTRAINT ck_device_endpoints_kind_id
        CHECK (metadata.is_valid_identifier(endpoint_kind_id)),

    CONSTRAINT ck_device_endpoints_name
        CHECK (
            char_length(display_name) BETWEEN 1 AND 200
            AND display_name = btrim(display_name)
            AND display_name !~ '[[:cntrl:]]'
        ),

    CONSTRAINT ck_device_endpoints_ordinal
        CHECK (endpoint_ordinal > 0),

    CONSTRAINT uq_device_endpoints_ordinal
        UNIQUE (tenant_id, device_asset_id, endpoint_ordinal)
);


-- ============================================================================
-- Documentation stored in PostgreSQL
-- ============================================================================

COMMENT ON TABLE metadata.communication_devices IS
    'Communication capability attached to an existing tenant-owned asset.';

COMMENT ON COLUMN metadata.communication_devices.device_asset_id IS
    'Asset identity reused as the communication device identity.';

COMMENT ON TABLE metadata.device_endpoints IS
    'Stable physical or logical observation sources exposed by communication devices.';

COMMENT ON COLUMN metadata.device_endpoints.endpoint_id IS
    'Stable endpoint identity scoped by the owning communication device.';

COMMENT ON COLUMN metadata.device_endpoints.endpoint_kind_id IS
    'Extensible endpoint category interpreted by device profiles and operator workflows.';

COMMENT ON COLUMN metadata.device_endpoints.display_name IS
    'Operator-facing endpoint label; not part of endpoint identity.';

COMMENT ON COLUMN metadata.device_endpoints.endpoint_ordinal IS
    'One-based declaration order retained when reconstructing the device aggregate.';
