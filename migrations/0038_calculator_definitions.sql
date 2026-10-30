-- Immutable declarations only: registration does not schedule or execute work.
CREATE SCHEMA analytics;
CREATE TABLE analytics.calculator_definitions (
    tenant_id TEXT NOT NULL REFERENCES metadata.tenants (tenant_id),
    calculator_id TEXT NOT NULL CHECK (metadata.is_valid_identifier(calculator_id)),
    calculator_version TEXT NOT NULL CHECK (metadata.is_valid_identifier(calculator_version)),
    implementation TEXT NOT NULL CHECK (implementation = 'window-mean-v1'),
    input_id TEXT NOT NULL,
    input_version BIGINT NOT NULL,
    output_id TEXT NOT NULL,
    output_version BIGINT NOT NULL,
    PRIMARY KEY (tenant_id, calculator_id, calculator_version),
    FOREIGN KEY (tenant_id, input_id, input_version)
        REFERENCES metadata.property_definitions (tenant_id, property_id, version),
    FOREIGN KEY (tenant_id, output_id, output_version)
        REFERENCES metadata.property_definitions (tenant_id, property_id, version)
);
COMMENT ON TABLE analytics.calculator_definitions IS
    'Tenant-owned immutable first-party release declarations. window-mean-v1 has one numeric input and decimal mean output with equal canonical units; window and lateness <= seven days, inputs <=1024, minimum samples 1..1024. No activation or runtime state.';
CREATE FUNCTION analytics.prevent_definition_mutation() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'Calculator definitions are immutable';
END;
$$;
CREATE TRIGGER calculator_definitions_immutable
BEFORE UPDATE OR DELETE ON analytics.calculator_definitions
FOR EACH ROW EXECUTE FUNCTION analytics.prevent_definition_mutation();
-- Enforce semantic compatibility even for direct SQL writers. Referenced
-- property versions are immutable, so this check cannot become stale later.
CREATE FUNCTION analytics.check_definition_properties() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM metadata.property_definitions i
        JOIN metadata.property_definitions o ON o.tenant_id = i.tenant_id
        WHERE i.tenant_id = NEW.tenant_id
          AND i.property_id = NEW.input_id AND i.version = NEW.input_version
          AND o.property_id = NEW.output_id AND o.version = NEW.output_version
          AND i.value_kind IN ('integer', 'decimal') AND o.value_kind = 'decimal'
          AND i.canonical_unit IS NOT DISTINCT FROM o.canonical_unit
    ) THEN
        RAISE EXCEPTION 'Calculator properties are missing or incompatible' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER calculator_definitions_validate_properties
BEFORE INSERT ON analytics.calculator_definitions
FOR EACH ROW EXECUTE FUNCTION analytics.check_definition_properties();
