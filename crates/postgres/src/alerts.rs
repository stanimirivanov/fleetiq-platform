//! Serialized evidence consumption, lifecycle checkpoint and transactional outbox.
use fleetiq_analytics::{AlertCondition, AlertLifecycle, AlertPhase};
use fleetiq_domain::UnixTimeMillis;
use sqlx::PgPool;

/// Trusted worker adapter; no operator authorization or network delivery surface.
pub struct PostgresAlertLifecycle(PgPool);
impl PostgresAlertLifecycle {
    /// Uses a checked-schema pool with bounded statement deadlines.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    /// Consumes at most one visible evaluation for one selected rule revision.
    /// Locks current tenant/activation before reading the checkpoint. Cancellation
    /// or any failed write rolls back receipt, state, transition and intent together.
    /// A new statement after the head lock observes the current pinned selection.
    /// # Errors
    /// Fails closed on malformed checkpoints, invalid evidence or storage failures.
    pub async fn advance_one(&self) -> Result<bool, AlertProcessingError> {
        let mut tx = self.0.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
            .execute(&mut *tx)
            .await?;
        let Some((tenant, rule)) = lock_candidate(&mut tx).await? else {
            return Ok(false);
        };
        let row = sqlx::query!(r#"
            SELECT a.revision,a.phase,a.candidate_since_ms,a.last_event_at_ms,
              a.activation_generation,a.version,h.generation,r.definition,
              GREATEST(a.live_from_ms,c.recorded_at_ms) AS "live_from_ms!",
              e.evaluation_id AS "evaluation_id!",e.start_ms AS "start_ms!",
              e.end_ms AS "end_ms!",e.evaluated_ms AS "evaluated_ms!",e.value,e.scheduled AS "scheduled!"
            FROM analytics.alert_rules h
            JOIN analytics.alert_rule_revisions r ON r.tenant_id=h.tenant_id AND r.rule_id=h.rule_id AND r.revision=h.active_revision
            JOIN analytics.alert_lifecycles a ON a.tenant_id=r.tenant_id AND a.rule_id=r.rule_id AND a.revision=r.revision
            JOIN analytics.alert_rule_activation_changes c ON c.tenant_id=h.tenant_id AND c.rule_id=h.rule_id AND c.generation=h.generation
            CROSS JOIN LATERAL analytics.next_alert_evidence(h.tenant_id,h.rule_id,h.active_revision) e
            WHERE h.tenant_id=$1 AND h.rule_id=$2 FOR UPDATE OF a
        "#,tenant,rule).fetch_one(&mut *tx).await?;
        let definition = crate::alert_rules::decode(&row.definition)
            .map_err(|_| AlertProcessingError::InvalidCheckpoint)?;
        let state = decode_checkpoint(&row.phase, row.candidate_since_ms, row.last_event_at_ms)?;
        let value = row
            .value
            .as_deref()
            .map(str::parse)
            .transpose()
            .map_err(|_| AlertProcessingError::InvalidCheckpoint)?;
        let condition = definition.condition(value.as_ref());
        let advanced = state.advance(
            condition,
            UnixTimeMillis::new(row.start_ms),
            UnixTimeMillis::new(row.end_ms),
            definition.hold_ms,
            definition.resolve_after_ms,
            row.activation_generation != row.generation,
        );
        // Historical input is auditable but cannot create a live phase or intent.
        // No max-sequence checkpoint: a producer that commits late is still seen.
        let (disposition, next) = if !row.scheduled
            || row.end_ms < row.live_from_ms
            || row.evaluated_ms < row.live_from_ms
        {
            ("historical", None)
        } else if let Some(next) = advanced {
            (
                if condition == AlertCondition::Missing {
                    "missing"
                } else {
                    "applied"
                },
                Some(next),
            )
        } else {
            ("out_of_order", None)
        };
        let changed = next.is_some_and(|n| n.phase() != state.phase());
        let next = next.unwrap_or(state);
        let version = row
            .version
            .checked_add(i64::from(changed))
            .ok_or(AlertProcessingError::InvalidCheckpoint)?;
        let generation = if matches!(disposition, "applied" | "missing") {
            row.generation
        } else {
            row.activation_generation
        };
        let phase = next.phase().as_str();
        let since = next.candidate_since().map(UnixTimeMillis::get);
        let last = next.last_event().map(UnixTimeMillis::get);
        sqlx::query!(r#"
            INSERT INTO analytics.alert_evaluation_receipts(tenant_id,rule_id,revision,evaluation_id,activation_generation,disposition)
            VALUES($1,$2,$3,$4,$5,$6)
        "#,&tenant,&rule,row.revision,row.evaluation_id,row.generation,disposition).execute(&mut *tx).await?;
        sqlx::query!(r#"
            UPDATE analytics.alert_lifecycles SET phase=$4,candidate_since_ms=$5,last_event_at_ms=$6,
              activation_generation=$7,version=$8,last_turn_at=clock_timestamp()
            WHERE tenant_id=$1 AND rule_id=$2 AND revision=$3
        "#,&tenant,&rule,row.revision,phase,since,last,generation,version).execute(&mut *tx).await?;
        if changed {
            sqlx::query!(r#"
                WITH transition AS (
                  INSERT INTO analytics.alert_transitions(tenant_id,rule_id,revision,version,evaluation_id,phase,event_at_ms)
                  VALUES($1,$2,$3,$4,$5,$6,$7) RETURNING tenant_id,rule_id,revision,version,phase
                ) INSERT INTO analytics.alert_notification_intents(tenant_id,rule_id,revision,transition_version)
                  SELECT tenant_id,rule_id,revision,version FROM transition WHERE phase IN ('firing','resolved')
            "#,&tenant,&rule,row.revision,version,row.evaluation_id,phase,row.end_ms).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(true)
    }
}
async fn lock_candidate(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<Option<(String, String)>, AlertProcessingError> {
    let candidate = sqlx::query!(r#"
            SELECT h.tenant_id,h.rule_id FROM analytics.alert_rules h
            JOIN metadata.tenants t USING(tenant_id)
            JOIN analytics.alert_lifecycles a ON a.tenant_id=h.tenant_id
              AND a.rule_id=h.rule_id AND a.revision=h.active_revision
            WHERE t.is_active AND NOT EXISTS(SELECT 1 FROM analytics.work_controls control WHERE control.tenant_id=h.tenant_id AND control.kind='alert_evaluation' AND control.paused) AND EXISTS(SELECT 1 FROM analytics.next_alert_evidence(h.tenant_id,h.rule_id,h.active_revision))
            ORDER BY a.last_turn_at,h.tenant_id,h.rule_id
            FOR SHARE OF t SKIP LOCKED FOR UPDATE OF h SKIP LOCKED LIMIT 1
        "#).fetch_optional(&mut **tx).await?;
    Ok(candidate.map(|c| (c.tenant_id, c.rule_id)))
}
fn decode_checkpoint(
    phase: &str,
    since: Option<i64>,
    last: Option<i64>,
) -> Result<AlertLifecycle, AlertProcessingError> {
    let phase = match phase {
        "inactive" => AlertPhase::Inactive,
        "pending" => AlertPhase::Pending,
        "firing" => AlertPhase::Firing,
        "resolved" => AlertPhase::Resolved,
        _ => return Err(AlertProcessingError::InvalidCheckpoint),
    };
    AlertLifecycle::restore(
        phase,
        since.map(UnixTimeMillis::new),
        last.map(UnixTimeMillis::new),
    )
    .ok_or(AlertProcessingError::InvalidCheckpoint)
}
/// Bounded worker failure with an inspectable internal storage cause.
#[derive(Debug)]
pub enum AlertProcessingError {
    /// Persisted typed content cannot be safely reconstructed.
    InvalidCheckpoint,
    /// Transaction failed; no partial lifecycle or notification intent commits.
    Database(sqlx::Error),
}
impl From<sqlx::Error> for AlertProcessingError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e)
    }
}
impl std::fmt::Display for AlertProcessingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("alert lifecycle processing failed; durable retry retained")
    }
}
impl std::error::Error for AlertProcessingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(e) => Some(e),
            Self::InvalidCheckpoint => None,
        }
    }
}
