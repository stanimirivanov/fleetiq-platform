//! Read-only bounded work projections. Opaque payloads/URLs never leave SQL.
use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};
use fleetiq_access::{AuthorizedTenant, ReadOperations};
use fleetiq_analytics::{WorkDiagnostics, WorkQueueEvidence};
use sqlx::PgPool;

/// Operational adapter, requiring exact action-typed authorization evidence.
pub struct PostgresWorkDiagnostics(PgPool);
impl PostgresWorkDiagnostics {
    /// Shares the bounded server pool.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    /// Samples at most 64 identities per family, using a 65th only for coverage.
    /// Alert candidates reuse the worker's exact selector; the database deadline
    /// bounds work inside that selector as well as the fixed number of calls.
    /// # Errors
    /// Fails closed on timeout, storage failure or corrupt closed vocabulary.
    pub async fn snapshot(
        &self,
        auth: AuthorizedTenant<ReadOperations>,
    ) -> Result<WorkDiagnostics, PostgresRepositoryError> {
        let mut tx = self.0.begin().await.map_err(classify_database_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(classify_database_error)?;
        sqlx::query("SET LOCAL statement_timeout = '2s'")
            .execute(&mut *tx)
            .await
            .map_err(classify_database_error)?;
        let execution = fleetiq_protocol_sdk::replay_wire::replay_fingerprint();
        let rows = sqlx::query!(r#"
WITH clock AS MATERIALIZED (SELECT floor(extract(epoch FROM transaction_timestamp())*1000)::bigint AS now),
replay AS MATERIALIZED (SELECT * FROM ingestion.replay_work WHERE tenant_id=$1 AND status<>'completed' ORDER BY message_id LIMIT 65),
calculator AS MATERIALIZED (SELECT * FROM analytics.calculator_work WHERE tenant_id=$1 AND status<>'done' ORDER BY evaluation_id LIMIT 65),
rules AS MATERIALIZED (SELECT * FROM analytics.alert_rules WHERE tenant_id=$1 AND active_revision IS NOT NULL ORDER BY rule_id LIMIT 65),
notification AS MATERIALIZED (SELECT * FROM analytics.alert_notification_delivery WHERE tenant_id=$1 ORDER BY destination,rule_id,revision,firing_version LIMIT 65),
positions AS (
 SELECT 'replay' AS kind,w.message_id AS id,w.generation AS attempts,
 CASE WHEN w.status='completed' THEN 'idle' WHEN w.status='poisoned' THEN 'failed'
 WHEN w.lease_until_ms>clock.now THEN 'leased' WHEN w.generation>=3 THEN 'failed'
 WHEN w.sequence<>s.current_sequence+1 OR s.execution_sha256<>$2 OR s.execution_policy<>1 OR s.admission_policy<>1 THEN 'blocked' ELSE 'eligible' END AS state,
 m.received_at_ms AS since,NULL::text AS failure
 FROM replay w JOIN ingestion.replay_sessions s USING(tenant_id,session_id)
 JOIN ingestion.accepted_messages m USING(tenant_id,message_id) CROSS JOIN clock
 UNION ALL
 SELECT 'calculator',w.evaluation_id,w.generation::bigint,
 CASE WHEN w.status='done' THEN 'idle' WHEN w.status='exhausted' THEN 'failed'
 WHEN w.lease_until_ms>clock.now THEN 'leased' WHEN w.generation>=3 THEN 'failed'
 WHEN NOT EXISTS(SELECT 1 FROM analytics.calculator_configuration_revisions r JOIN analytics.calculator_configurations c
 ON (c.tenant_id,c.configuration_id,c.active_revision)=(r.tenant_id,r.configuration_id,r.revision)
 WHERE r.tenant_id=w.tenant_id AND r.schedule_id=w.schedule_id) THEN 'blocked' ELSE 'eligible' END,
 (w.start_ms::numeric+s.window_ms+s.lateness_ms)::bigint,NULL::text
 FROM calculator w JOIN analytics.calculator_schedules s USING(tenant_id,schedule_id) CROSS JOIN clock
 UNION ALL
 SELECT 'alert_evaluation',h.rule_id,0::bigint,CASE WHEN e.evaluation_id IS NULL THEN 'idle' ELSE 'eligible' END,e.evaluated_ms,NULL::text
 FROM rules h LEFT JOIN LATERAL analytics.next_alert_evidence(h.tenant_id,h.rule_id,h.active_revision) e ON true
 UNION ALL
 SELECT 'notification',encode(d.destination,'hex')||':'||d.rule_id||':'||d.revision||':'||d.firing_version,d.attempts::bigint,
 CASE WHEN d.lease_until>transaction_timestamp() THEN 'leased'
 WHEN n.transition_version>d.target_version THEN 'eligible'
 WHEN d.attempts>=5 THEN 'failed'
 WHEN d.accepted_version=d.target_version AND d.resolved_until<=transaction_timestamp() THEN 'idle'
 WHEN d.next_attempt_at>transaction_timestamp() THEN 'deferred' ELSE 'eligible' END,
 floor(extract(epoch FROM CASE WHEN n.transition_version>d.target_version THEN n.recorded_at ELSE d.next_attempt_at END)*1000)::bigint,
 CASE d.last_failure WHEN 'transport' THEN 'transport' WHEN 'rejected' THEN 'rejected'
 WHEN 'response_limit' THEN 'response_limit' WHEN 'invalid_payload' THEN 'invalid_payload' END
 FROM notification d LEFT JOIN LATERAL (SELECT transition_version,recorded_at FROM analytics.alert_notification_intents i
 WHERE (i.tenant_id,i.rule_id,i.revision)=(d.tenant_id,d.rule_id,d.revision) AND i.transition_version>d.firing_version ORDER BY transition_version LIMIT 1) n ON true
)
SELECT clock.now AS "now!",COALESCE(p.kind,'') AS "kind!",COALESCE(p.attempts,0) AS "attempts!",COALESCE(p.state,'') AS "state!",p.since,p.failure
FROM clock LEFT JOIN positions p ON true ORDER BY p.kind,p.id
        "#, auth.tenant_id().as_str(), execution.as_slice()).fetch_all(&mut *tx).await.map_err(classify_database_error)?;
        let now = rows.first().map_or(0, |r| r.now);
        let mut queues: Vec<_> = ["replay", "calculator", "alert_evaluation", "notification"]
            .map(WorkQueueEvidence::empty)
            .into();
        for row in rows {
            // LEFT JOIN emits one all-null position for a completely empty tenant.
            if row.kind.is_empty() {
                continue;
            }
            let queue = queues
                .iter_mut()
                .find(|q| q.kind == row.kind)
                .ok_or_else(|| corrupt("work kind", &row.kind))?;
            if queue.sampled == 64 {
                queue.truncated = true;
                continue;
            }
            observe(
                queue,
                row.attempts,
                &row.state,
                row.since,
                row.failure.as_deref(),
            )?;
        }
        for queue in &mut queues {
            queue.failure_categories.sort_unstable();
        }
        tx.commit().await.map_err(classify_database_error)?;
        Ok(WorkDiagnostics {
            observed_at_ms: now,
            queues,
        })
    }
}

fn observe(
    queue: &mut WorkQueueEvidence,
    attempts: i64,
    state: &str,
    since: Option<i64>,
    failure: Option<&str>,
) -> Result<(), PostgresRepositoryError> {
    queue.sampled += 1;
    queue.max_attempts = queue
        .max_attempts
        .max(u32::try_from(attempts).map_err(|_| corrupt("attempts", "invalid"))?);
    match state {
        "eligible" => {
            queue.eligible += 1;
            if let Some(t) = since {
                queue.oldest_sampled_eligible_at_ms = Some(
                    queue
                        .oldest_sampled_eligible_at_ms
                        .map_or(t, |old| old.min(t)),
                );
            }
        }
        "leased" => queue.leased += 1,
        "failed" => queue.failed += 1,
        "blocked" => queue.blocked += 1,
        "deferred" => queue.deferred += 1,
        "idle" => {}
        _ => return Err(corrupt("work state", "invalid")),
    }
    let failure = match failure {
        Some("transport") => Some("transport"),
        Some("rejected") => Some("rejected"),
        Some("response_limit") => Some("response_limit"),
        Some("invalid_payload") => Some("invalid_payload"),
        None if state == "failed"
            && attempts >= if queue.kind == "notification" { 5 } else { 3 } =>
        {
            Some("attempts_exhausted")
        }
        None => None,
        _ => return Err(corrupt("work failure", "invalid")),
    };
    if let Some(f) = failure
        && !queue.failure_categories.contains(&f)
    {
        queue.failure_categories.push(f);
    }
    Ok(())
}
