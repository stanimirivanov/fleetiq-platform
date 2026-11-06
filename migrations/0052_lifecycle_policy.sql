-- Additive, empty policy ledger. No backfill, expiry or deletion behavior.
-- Short schema locks only; existing history and worker tables are untouched.
CREATE TABLE history.lifecycle_policies (
    tenant_id TEXT NOT NULL REFERENCES metadata.tenants(tenant_id),
    revision BIGINT NOT NULL CHECK (revision > 0),
    actor_id TEXT NOT NULL REFERENCES identity.actors(actor_id),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    hot_days INTEGER,
    archive_days INTEGER,
    max_jobs INTEGER,
    max_bytes BIGINT,
    CONSTRAINT pk_lifecycle_policies PRIMARY KEY (tenant_id, revision),
    CONSTRAINT ck_lifecycle_policies_limits CHECK (
        (hot_days IS NULL AND archive_days IS NULL AND max_jobs IS NULL AND max_bytes IS NULL)
        OR (hot_days IS NOT NULL AND archive_days IS NOT NULL AND max_jobs IS NOT NULL AND max_bytes IS NOT NULL
            AND hot_days > 0 AND archive_days >= hot_days AND max_jobs > 0 AND max_bytes > 0))
);
COMMENT ON TABLE history.lifecycle_policies IS 'Append-only tenant policy revisions; absence or all-null limits means disabled. No automatic expiry or deletion authority.';
CREATE FUNCTION history.prevent_lifecycle_policy_mutation() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'lifecycle policy revisions are immutable';
END;
$$;
CREATE TRIGGER lifecycle_policies_immutable BEFORE UPDATE OR DELETE ON history.lifecycle_policies
FOR EACH ROW EXECUTE FUNCTION history.prevent_lifecycle_policy_mutation();
