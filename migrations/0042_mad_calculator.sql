-- Admit one additional immutable first-party ABI. Existing mean releases retain
-- their exact meaning and same-unit constraint; MAD emits a dimensionless 0/1
-- decimal flag from a numeric input. No data rewrite, activation or new privileges.
ALTER TABLE analytics.calculator_definitions
    DROP CONSTRAINT calculator_definitions_implementation_check,
    ADD CONSTRAINT ck_calculator_definitions_implementation
        CHECK (implementation IN ('window-mean-v1','window-mad-v1'));

-- Enforce per-ABI units for trusted SQL writers as well as registry callers.
CREATE OR REPLACE FUNCTION analytics.check_definition_properties() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM metadata.property_definitions i
        JOIN metadata.property_definitions o ON o.tenant_id = i.tenant_id
        WHERE i.tenant_id = NEW.tenant_id
          AND i.property_id = NEW.input_id AND i.version = NEW.input_version
          AND o.property_id = NEW.output_id AND o.version = NEW.output_version
          AND i.value_kind IN ('integer', 'decimal') AND o.value_kind = 'decimal'
          AND ((NEW.implementation='window-mean-v1' AND i.canonical_unit IS NOT DISTINCT FROM o.canonical_unit)
            OR (NEW.implementation='window-mad-v1' AND o.canonical_unit IS NULL))
    ) THEN
        RAISE EXCEPTION 'Calculator properties are missing or incompatible' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;
COMMENT ON FUNCTION analytics.check_definition_properties() IS
    'Exact tenant-owned numeric input and decimal output; mean preserves canonical units, MAD requires dimensionless output.';
COMMENT ON TABLE analytics.calculator_definitions IS
    'Immutable reviewed releases: window-mean-v1 preserves units; window-mad-v1 compares newest point to preceding median with strict deviation > 6 raw MAD, >=3 baseline samples, zero-MAD departure policy, dimensionless decimal 0/1 output. <=1024 inputs; no activation.';
