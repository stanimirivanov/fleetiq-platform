//! Destination-local incident queue with finite retries and restart-safe refresh.
use fleetiq_analytics::{AlertDeliveryFailure, AlertNotificationLease, AlertPhase, AlertRuleId};
use fleetiq_domain::{TenantId, UnixTimeMillis};
use sqlx::PgPool;
/// Trusted worker adapter; no HTTP or operator-command authority is implied.
pub struct PostgresAlertDelivery(PgPool);
impl PostgresAlertDelivery {
    /// Uses a checked-schema pool with bounded statement deadlines.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    /// Seeds at most 32 retained firing incidents for this endpoint. Resolutions
    /// are selected afresh at claim time, so old firing intents cannot reopen them.
    /// # Errors
    /// Reports bounded storage failure; no intent or lifecycle fact is rewritten.
    pub async fn seed(&self, destination: &[u8; 32]) -> Result<(), AlertDeliveryError> {
        sqlx::query!(r#"
          INSERT INTO analytics.alert_notification_delivery(destination,tenant_id,rule_id,revision,firing_version,target_version)
          SELECT $1,i.tenant_id,i.rule_id,i.revision,i.transition_version,i.transition_version
          FROM analytics.alert_notification_intents i JOIN analytics.alert_transitions t
            ON (t.tenant_id,t.rule_id,t.revision,t.version)=(i.tenant_id,i.rule_id,i.revision,i.transition_version)
          JOIN metadata.tenants tenant ON tenant.tenant_id=i.tenant_id
          WHERE tenant.is_active AND t.phase='firing' AND NOT EXISTS(
            SELECT 1 FROM analytics.alert_notification_delivery d WHERE d.destination=$1
            AND (d.tenant_id,d.rule_id,d.revision,d.firing_version)=(i.tenant_id,i.rule_id,i.revision,i.transition_version))
          ORDER BY i.recorded_at,i.tenant_id,i.rule_id,i.revision,i.transition_version LIMIT 32 ON CONFLICT DO NOTHING
        "#,destination.as_slice()).execute(&self.0).await.map_err(|_|AlertDeliveryError)?;
        Ok(())
    }
    /// Claims one incident, serializing only its short database transaction.
    /// A newer resolution rearms even an exhausted firing cycle. Active refresh
    /// and resolved healing share the same monotonic fence and finite lease.
    /// # Errors
    /// Malformed identities, immutable definitions or storage fail closed.
    pub async fn claim(
        &self,
        destination: &[u8; 32],
    ) -> Result<Option<AlertNotificationLease>, AlertDeliveryError> {
        let row=sqlx::query!(r#"
          WITH candidate AS (
            SELECT d.tenant_id,d.rule_id,d.revision,d.firing_version,
              COALESCE(n.version,f.version) AS version,COALESCE(n.phase,f.phase) AS phase,
              f.event_at_ms AS starts_ms,n.event_at_ms AS resolved_ms,r.definition
            FROM analytics.alert_notification_delivery d
            JOIN metadata.tenants tenant USING(tenant_id)
            JOIN analytics.alert_rule_revisions r ON (r.tenant_id,r.rule_id,r.revision)=(d.tenant_id,d.rule_id,d.revision)
            JOIN analytics.alert_transitions f ON (f.tenant_id,f.rule_id,f.revision,f.version)=(d.tenant_id,d.rule_id,d.revision,d.firing_version)
            LEFT JOIN LATERAL (
              SELECT t.version,t.phase,t.event_at_ms FROM analytics.alert_notification_intents i
              JOIN analytics.alert_transitions t ON (t.tenant_id,t.rule_id,t.revision,t.version)=(i.tenant_id,i.rule_id,i.revision,i.transition_version)
              WHERE (i.tenant_id,i.rule_id,i.revision)=(d.tenant_id,d.rule_id,d.revision) AND i.transition_version>d.firing_version
              ORDER BY i.transition_version LIMIT 1
            ) n ON true
            WHERE d.destination=$1 AND tenant.is_active AND f.phase='firing' AND (n.phase IS NULL OR n.phase='resolved')
              AND d.generation<9223372036854775807 AND (d.lease_until IS NULL OR d.lease_until<=clock_timestamp())
              AND (COALESCE(n.version,f.version)>d.target_version OR (d.attempts<5 AND d.next_attempt_at<=clock_timestamp()
                AND (d.accepted_version<d.target_version OR n.phase IS NULL OR d.resolved_until>clock_timestamp())))
            ORDER BY d.next_attempt_at,d.tenant_id,d.rule_id,d.revision,d.firing_version
            FOR SHARE OF tenant SKIP LOCKED FOR UPDATE OF d SKIP LOCKED LIMIT 1
          ) UPDATE analytics.alert_notification_delivery d SET target_version=c.version,generation=d.generation+1,
            attempts=CASE WHEN c.version>d.target_version THEN 1 ELSE d.attempts+1 END,
            lease_until=clock_timestamp()+interval '30 seconds'
          FROM candidate c WHERE d.destination=$1 AND (d.tenant_id,d.rule_id,d.revision,d.firing_version)=(c.tenant_id,c.rule_id,c.revision,c.firing_version)
          RETURNING d.tenant_id,d.rule_id,d.revision,d.firing_version,d.target_version,d.generation,
            c.phase AS "phase!",c.starts_ms AS "starts_ms!",c.resolved_ms AS "resolved_ms?",c.definition AS "definition!",
            (floor(extract(epoch FROM clock_timestamp())*1000)::bigint+120000) AS "active_until!"
        "#,destination.as_slice()).fetch_optional(&self.0).await.map_err(|_|AlertDeliveryError)?;
        row.map(|r| {
            Ok(AlertNotificationLease {
                destination: *destination,
                tenant: TenantId::new(r.tenant_id).map_err(|_| AlertDeliveryError)?,
                rule: AlertRuleId::new(r.rule_id).map_err(|_| AlertDeliveryError)?,
                revision: u32::try_from(r.revision).map_err(|_| AlertDeliveryError)?,
                firing_version: r.firing_version,
                version: r.target_version,
                generation: r.generation,
                phase: if r.phase == "firing" {
                    AlertPhase::Firing
                } else {
                    AlertPhase::Resolved
                },
                starts_at: UnixTimeMillis::new(r.starts_ms),
                ends_at: UnixTimeMillis::new(r.resolved_ms.unwrap_or(r.active_until)),
                definition: crate::alert_rules::decode(&r.definition)
                    .map_err(|_| AlertDeliveryError)?,
            })
        })
        .transpose()
    }
    /// Records only a current unexpired snapshot. Late responses cannot advance a
    /// replacement generation. Acceptance is remote HTTP receipt, not delivery.
    /// # Errors
    /// Returns bounded storage errors; false means expired/stale/inactive ownership.
    pub async fn finish(
        &self,
        lease: &AlertNotificationLease,
        outcome: Result<(), AlertDeliveryFailure>,
    ) -> Result<bool, AlertDeliveryError> {
        let accepted = outcome.is_ok();
        let failure = outcome.err().map(AlertDeliveryFailure::as_str);
        let saved=sqlx::query!(r#"
          WITH tenant AS (
            SELECT tenant_id FROM metadata.tenants WHERE tenant_id=$2 AND is_active FOR SHARE
          ) UPDATE analytics.alert_notification_delivery d SET
            accepted_version=CASE WHEN $8 THEN d.target_version ELSE d.accepted_version END,
            attempts=CASE WHEN $8 THEN 0 ELSE d.attempts END,lease_until=NULL,last_failure=$9,
            next_attempt_at=clock_timestamp()+CASE WHEN $8 THEN interval '30 seconds' ELSE make_interval(secs=>LEAST(30,power(2,d.attempts)::int)) END,
            resolved_until=CASE WHEN $8 AND $10 THEN COALESCE(d.resolved_until,clock_timestamp()+interval '3 minutes') ELSE d.resolved_until END
          FROM tenant WHERE d.destination=$1 AND d.tenant_id=$2 AND d.rule_id=$3 AND d.revision=$4
            AND d.firing_version=$5 AND d.target_version=$6 AND d.generation=$7 AND d.lease_until>clock_timestamp()
            AND tenant.tenant_id=d.tenant_id
          RETURNING d.generation
        "#,lease.destination.as_slice(),lease.tenant.as_str(),lease.rule.as_str(),i64::from(lease.revision),lease.firing_version,lease.version,lease.generation,accepted,failure,lease.phase==AlertPhase::Resolved)
            .fetch_optional(&self.0).await.map_err(|_|AlertDeliveryError)?;
        Ok(saved.is_some())
    }
}
/// Bounded durable notification storage/reconstruction failure.
#[derive(Debug)]
pub struct AlertDeliveryError;
impl std::fmt::Display for AlertDeliveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("alert delivery storage unavailable")
    }
}
impl std::error::Error for AlertDeliveryError {}
