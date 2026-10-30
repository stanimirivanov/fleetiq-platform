-- A preparation commits before calculation, so late arrivals and process crashes
-- cannot change the original snapshot. No scheduler or activation is introduced.
CREATE TABLE analytics.prepared_evaluations (
    tenant_id TEXT NOT NULL,
    evaluation_id TEXT NOT NULL CHECK(metadata.is_valid_identifier(evaluation_id)),
    request JSONB NOT NULL CHECK(jsonb_typeof(request)='object' AND octet_length(request::text)<=4096),
    selection JSONB NOT NULL CHECK(jsonb_typeof(selection)='object' AND octet_length(selection::text)<=8388608),
    PRIMARY KEY(tenant_id,evaluation_id),
    FOREIGN KEY(tenant_id) REFERENCES metadata.tenants(tenant_id)
);
CREATE TRIGGER prepared_evaluations_immutable BEFORE UPDATE OR DELETE ON analytics.prepared_evaluations
FOR EACH ROW EXECUTE FUNCTION analytics.prevent_definition_mutation();
COMMENT ON TABLE analytics.prepared_evaluations IS
    'Immutable host preparation, exact request and canonical samples committed before calculation. Direct writes require trusted host privileges; terminal append revalidates durable semantic/target references. No arbitrary code or recursive calculated inputs.';
