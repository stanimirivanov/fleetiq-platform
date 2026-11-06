-- Additive short-lock migration; no history rewrite or implicit policy activation.
CREATE TABLE history.work_reservations (
    reservation_id BIGINT GENERATED ALWAYS AS IDENTITY,
    tenant_id TEXT NOT NULL,
    policy_revision BIGINT NOT NULL,
    reserved_bytes BIGINT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (clock_timestamp() + interval '30 seconds'),
    CONSTRAINT pk_work_reservations PRIMARY KEY (tenant_id,reservation_id),
    CONSTRAINT fk_work_reservations_policy FOREIGN KEY (tenant_id,policy_revision)
        REFERENCES history.lifecycle_policies(tenant_id,revision),
    CONSTRAINT ck_work_reservations_bytes CHECK (reserved_bytes > 0)
);
CREATE INDEX ix_work_reservations_expiry ON history.work_reservations(tenant_id,expires_at);
COMMENT ON TABLE history.work_reservations IS 'Transient tenant slot/byte reservations; only bounded work may use this 30-second lease. Expiry is reconciled on subsequent admission; no raw requests or telemetry.';
