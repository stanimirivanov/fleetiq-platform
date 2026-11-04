-- Destination-scoped leased delivery of immutable alert intents. A firing version
-- identifies one incident; later resolution supersedes unsent firing snapshots.
-- No network action occurs during migration, HTTP startup or lifecycle writes.
CREATE TABLE analytics.alert_notification_delivery (
    destination BYTEA NOT NULL,
    tenant_id TEXT NOT NULL,
    rule_id TEXT NOT NULL,
    revision BIGINT NOT NULL,
    firing_version BIGINT NOT NULL,
    target_version BIGINT NOT NULL,
    accepted_version BIGINT NOT NULL DEFAULT 0,
    generation BIGINT NOT NULL DEFAULT 0,
    attempts INTEGER NOT NULL DEFAULT 0,
    lease_until TIMESTAMPTZ,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    -- After first accepted resolution, heal late/ambiguous active requests for
    -- three minutes, longer than their two-minute advertised remote validity.
    resolved_until TIMESTAMPTZ,
    last_failure TEXT,
    CONSTRAINT pk_alert_notification_delivery PRIMARY KEY(destination,tenant_id,rule_id,revision,firing_version),
    CONSTRAINT fk_alert_delivery_firing FOREIGN KEY(tenant_id,rule_id,revision,firing_version)
      REFERENCES analytics.alert_notification_intents(tenant_id,rule_id,revision,transition_version),
    CONSTRAINT fk_alert_delivery_target FOREIGN KEY(tenant_id,rule_id,revision,target_version)
      REFERENCES analytics.alert_notification_intents(tenant_id,rule_id,revision,transition_version),
    CONSTRAINT ck_alert_delivery_destination CHECK(octet_length(destination)=32),
    CONSTRAINT ck_alert_delivery_versions CHECK(firing_version>0 AND target_version>=firing_version AND accepted_version BETWEEN 0 AND target_version AND generation>=0),
    CONSTRAINT ck_alert_delivery_attempts CHECK(attempts BETWEEN 0 AND 5),
    CONSTRAINT ck_alert_delivery_failure CHECK(last_failure IN ('transport','rejected','response_limit','invalid_payload'))
);
-- A due-time index supports periodic refresh/retry across independent incidents.
CREATE INDEX ix_alert_notification_delivery_due ON analytics.alert_notification_delivery(destination,next_attempt_at);
COMMENT ON TABLE analytics.alert_notification_delivery IS 'Mutable worker state per destination digest and firing incident. Intents/transitions remain immutable; acceptance means only the remote HTTP endpoint accepted a snapshot.';
COMMENT ON COLUMN analytics.alert_notification_delivery.destination IS 'SHA256 of canonical operator-configured endpoint URL. A new endpoint gets independent replay/acceptance state.';
COMMENT ON COLUMN analytics.alert_notification_delivery.attempts IS 'At most five attempts per target/refresh cycle; expired leases count. Success or a newer resolution starts a new cycle. Exhaustion is retained for later operational recovery.';
COMMENT ON COLUMN analytics.alert_notification_delivery.resolved_until IS 'Fixed database-clock reconciliation horizon after first accepted resolution; refresh does not extend it.';
-- Preserve identities and accepted history. No consumer can repurpose or delete
-- a delivery position; only its monotonic fence/target and retry state may evolve.
CREATE FUNCTION analytics.protect_alert_delivery() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Alert delivery identity is retained'; END IF;
    IF ROW(NEW.destination,NEW.tenant_id,NEW.rule_id,NEW.revision,NEW.firing_version)
       IS DISTINCT FROM ROW(OLD.destination,OLD.tenant_id,OLD.rule_id,OLD.revision,OLD.firing_version)
       OR NEW.generation<OLD.generation OR NEW.target_version<OLD.target_version OR NEW.accepted_version<OLD.accepted_version
    THEN RAISE EXCEPTION 'Alert delivery identity/fence cannot regress'; END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER alert_notification_delivery_identity BEFORE UPDATE OR DELETE ON analytics.alert_notification_delivery
  FOR EACH ROW EXECUTE FUNCTION analytics.protect_alert_delivery();
COMMENT ON FUNCTION analytics.protect_alert_delivery() IS 'Retains destination/tenant/incident identity and monotonic target, acceptance and lease generations.';
