//! Current-authority, fenced recovery commands and bounded command preparation.
use fleetiq_access::{AuthorizedTenant, ManageOperations, ReadOperations};
use fleetiq_domain::{ActorId, UnixTimeMillis};
use serde_json::Value;
use sqlx::PgPool;

/// Exact original receipt; repeated commands do not reapply old effects.
#[derive(Debug)]
pub struct WorkRecoveryReceipt {
    /// Generation produced by the original command, not a current-state assertion.
    pub generation: i64,
    /// Original authenticated actor.
    pub actor: ActorId,
    /// Original database receipt time.
    pub recorded_at: UnixTimeMillis,
}
/// Bounded storage projection of currently retryable positions and queue control.
#[derive(Debug)]
pub struct WorkRecoveryCandidates {
    /// Current queue-control fence; zero if no control has been written.
    pub generation: i64,
    /// Whether compatible worker candidate selection is paused.
    pub paused: bool,
    /// Persisted safe identity keys paired with current work fences; at most 32.
    pub targets: Vec<(Value, i64)>,
    /// Additional eligible positions were omitted.
    pub truncated: bool,
}
/// Operational command adapter; worker completion remains separately fenced.
pub struct PostgresWorkRecovery(PgPool);
impl PostgresWorkRecovery {
    /// Uses the server's bounded checked-schema pool.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    /// Applies one command or returns its original receipt after fresh authority checks.
    /// # Errors
    /// Distinguishes denial, collision/ineligible work and redacted storage failure.
    pub async fn apply(
        &self,
        auth: AuthorizedTenant<ManageOperations>,
        request: &fleetiq_analytics::WorkRecoveryId,
        kind: &str,
        operation: &str,
        target: &Value,
        expected: i64,
    ) -> Result<WorkRecoveryReceipt, WorkRecoveryError> {
        let mut tx = self.0.begin().await.map_err(classify)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        sqlx::query("SET LOCAL statement_timeout = '3s'")
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        let execution = fleetiq_protocol_sdk::replay_wire::replay_fingerprint();
        let row=sqlx::query!(r#"SELECT r.generation AS "generation!",r.actor_id AS "actor!",floor(extract(epoch FROM r.recorded_at)*1000)::bigint AS "recorded!"
            FROM analytics.recover_work($1,$2,$3,$4,$5,$6,$7,$8) r"#,
            auth.tenant_id().as_str(),auth.actor_id().as_str(),request.as_str(),kind,operation,target,expected,execution.as_slice())
            .fetch_one(&mut *tx).await.map_err(classify)?;
        let receipt = WorkRecoveryReceipt {
            generation: row.generation,
            actor: ActorId::new(row.actor).map_err(|_| WorkRecoveryError::InvalidReceipt)?,
            recorded_at: UnixTimeMillis::new(row.recorded),
        };
        tx.commit().await.map_err(classify)?;
        Ok(receipt)
    }
    /// Lists at most 32 eligible targets and their current control generation.
    /// Both read and manage evidence must belong to the same caller and tenant.
    /// # Errors
    /// No partial result is returned on deadline, storage or authorization failure.
    pub async fn inspect(
        &self,
        manage: AuthorizedTenant<ManageOperations>,
        read: AuthorizedTenant<ReadOperations>,
        kind: &str,
    ) -> Result<WorkRecoveryCandidates, WorkRecoveryError> {
        if manage.tenant_id() != read.tenant_id() || manage.actor_id() != read.actor_id() {
            return Err(WorkRecoveryError::Forbidden);
        }
        let mut tx = self.0.begin().await.map_err(classify)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        sqlx::query("SET LOCAL statement_timeout = '2s'")
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        let execution = fleetiq_protocol_sdk::replay_wire::replay_fingerprint();
        let rows=sqlx::query!(r#"
WITH replay AS MATERIALIZED (
 SELECT jsonb_build_object('id',w.message_id) AS target,w.generation FROM ingestion.replay_work w JOIN ingestion.replay_sessions s USING(tenant_id,session_id)
 WHERE $2='replay' AND w.tenant_id=$1 AND (w.status='poisoned' OR (w.status='pending' AND w.attempts=3))
 AND (w.lease_until_ms IS NULL OR w.lease_until_ms<=floor(extract(epoch FROM transaction_timestamp())*1000)::bigint)
 AND w.generation<=9223372036854775807-4 AND s.current_sequence+1=w.sequence AND s.execution_sha256=$3 AND s.execution_policy=1 AND s.admission_policy=1
 ORDER BY w.message_id LIMIT 33),
calculator AS MATERIALIZED (
 SELECT jsonb_build_object('id',w.evaluation_id) AS target,w.generation::bigint FROM analytics.calculator_work w
 WHERE $2='calculator' AND w.tenant_id=$1 AND (w.status='exhausted' OR (w.status='pending' AND w.attempts=3))
 AND (w.lease_until_ms IS NULL OR w.lease_until_ms<=floor(extract(epoch FROM transaction_timestamp())*1000)::bigint)
 AND w.generation<=2147483647-4 AND EXISTS(SELECT 1 FROM analytics.calculator_configuration_revisions r JOIN analytics.calculator_configurations c
 ON (c.tenant_id,c.configuration_id,c.active_revision)=(r.tenant_id,r.configuration_id,r.revision) WHERE r.tenant_id=w.tenant_id AND r.schedule_id=w.schedule_id)
 ORDER BY w.evaluation_id LIMIT 33),
notification AS MATERIALIZED (
 SELECT jsonb_build_object('destination',encode(d.destination,'hex'),'rule_id',d.rule_id,'revision',d.revision::text,'firing_version',d.firing_version::text) AS target,d.generation
 FROM analytics.alert_notification_delivery d WHERE $2='notification' AND d.tenant_id=$1 AND d.attempts=5 AND d.generation<=9223372036854775807-6
 AND (d.lease_until IS NULL OR d.lease_until<=transaction_timestamp())
 AND (d.accepted_version<d.target_version OR d.resolved_until IS NULL OR d.resolved_until>transaction_timestamp())
 AND NOT COALESCE((SELECT min(i.transition_version)>d.target_version FROM analytics.alert_notification_intents i
 WHERE (i.tenant_id,i.rule_id,i.revision)=(d.tenant_id,d.rule_id,d.revision) AND i.transition_version>d.firing_version),false)
 ORDER BY d.destination,d.rule_id,d.revision,d.firing_version LIMIT 33)
SELECT target AS "target!",generation AS "generation!" FROM replay UNION ALL SELECT * FROM calculator UNION ALL SELECT * FROM notification
        "#,manage.tenant_id().as_str(),kind,execution.as_slice()).fetch_all(&mut *tx).await.map_err(classify)?;
        let control=sqlx::query!(r#"SELECT COALESCE((SELECT generation FROM analytics.work_controls WHERE tenant_id=$1 AND kind=$2),0) AS "generation!",
          COALESCE((SELECT paused FROM analytics.work_controls WHERE tenant_id=$1 AND kind=$2),false) AS "paused!""#,manage.tenant_id().as_str(),kind).fetch_one(&mut *tx).await.map_err(classify)?;
        let truncated = rows.len() > 32;
        let targets = rows
            .into_iter()
            .take(32)
            .map(|r| (r.target, r.generation))
            .collect();
        tx.commit().await.map_err(classify)?;
        Ok(WorkRecoveryCandidates {
            generation: control.generation,
            paused: control.paused,
            targets,
            truncated,
        })
    }
}
/// Closed command outcome with no database diagnostics or target contents.
#[derive(Debug)]
pub enum WorkRecoveryError {
    /// Current actor/grant/tenant authority is absent.
    Forbidden,
    /// Identity collision, stale generation or ineligible target.
    Conflict,
    /// Storage failed; retry the same exact command identity.
    Storage(sqlx::Error),
    /// A persisted receipt cannot be reconstructed through native invariants.
    InvalidReceipt,
}
fn classify(e: sqlx::Error) -> WorkRecoveryError {
    match e
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .as_deref()
    {
        Some("42501") => WorkRecoveryError::Forbidden,
        Some("23505" | "23514" | "23503" | "22003") => WorkRecoveryError::Conflict,
        _ => WorkRecoveryError::Storage(e),
    }
}

impl std::fmt::Display for WorkRecoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("work recovery unavailable or conflicting")
    }
}
impl std::error::Error for WorkRecoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(e) => Some(e),
            _ => None,
        }
    }
}
