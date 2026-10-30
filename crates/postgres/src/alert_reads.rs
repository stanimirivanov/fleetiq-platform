//! Authorized read-only queue, detail and immutable transition projections.
use crate::AlertAcknowledgement;
use fleetiq_access::{AuthorizedTenant, ReadAlerts};
use fleetiq_analytics::{
    AlertLifecycle, AlertPhase, AlertRuleDefinition, AlertRuleId, CalculatorEvaluationId,
};
use fleetiq_domain::{ActorId, UnixTimeMillis};
use serde_json::Value;
use sqlx::PgPool;
/// Retained transition with exact source identity; no raw calculator payload.
pub struct AlertTransitionRead {
    /// Positive phase-transition version.
    pub version: i64,
    /// Original resulting lifecycle phase.
    pub phase: AlertPhase,
    /// Source calculator window end.
    pub event_at: UnixTimeMillis,
    /// Original database receipt instant.
    pub recorded_at: UnixTimeMillis,
    /// Activation selection when evidence was consumed.
    pub activation_generation: u32,
    /// Exact immutable evaluation identity within the selected rule scope.
    pub evaluation: CalculatorEvaluationId,
    /// Whether the selected output was usable, rather than explicit unknown evidence.
    pub output_available: bool,
    /// Independent current acknowledgement annotation for this exact version.
    pub acknowledgement: Option<AlertAcknowledgement>,
}
/// One observed revision-local alert and its pinned condition.
pub struct AlertRead {
    /// Rule series identity, qualified by the authorization tenant.
    pub rule: AlertRuleId,
    /// Positive immutable rule revision.
    pub revision: u32,
    /// Current event-time lifecycle checkpoint.
    pub state: AlertLifecycle,
    /// Current phase-transition version.
    pub version: i64,
    /// Whether this exact revision is currently selected for processing.
    pub selected: bool,
    /// Retained exact condition, including its scope/target/output identity.
    pub definition: AlertRuleDefinition,
    /// Latest retained phase transition from the same statement snapshot.
    pub latest: AlertTransitionRead,
}
struct Row {
    transition_phase: String,
    rule_id: String,
    revision: i64,
    phase: String,
    version: i64,
    candidate_since_ms: Option<i64>,
    last_event_at_ms: Option<i64>,
    selected: bool,
    definition: Value,
    event_at_ms: i64,
    recorded_at_ms: i64,
    activation_generation: i64,
    evaluation_id: String,
    disposition: String,
    ack_actor_id: Option<String>,
    ack_recorded_at_ms: Option<i64>,
}
struct TransitionRow {
    version: i64,
    phase: String,
    event_at_ms: i64,
    recorded_at_ms: i64,
    activation_generation: i64,
    evaluation_id: String,
    disposition: String,
    ack_actor_id: Option<String>,
    ack_recorded_at_ms: Option<i64>,
}
/// Existing alert history adapter; every read consumes explicit alert authority.
pub struct PostgresAlertReads(PgPool);
impl PostgresAlertReads {
    /// Uses a checked-schema pool without writing or running migrations.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    /// Reads limit+1 observed alerts in stable C-collated rule/revision order.
    /// Omitted phase means pending/firing. Filtering is current, not a snapshot.
    /// # Errors
    /// Rejects invalid limits/positions and fails closed on malformed persisted state.
    pub async fn list(
        &self,
        auth: &AuthorizedTenant<ReadAlerts>,
        phase: Option<AlertPhase>,
        limit: u16,
        after: Option<(AlertRuleId, u32)>,
    ) -> Result<Vec<AlertRead>, AlertReadError> {
        if !(1..=100).contains(&limit) || after.as_ref().is_some_and(|(_, r)| *r == 0) {
            return Err(AlertReadError::Invalid);
        }
        let (id, revision) = after.map_or((None, None), |(id, r)| {
            (Some(id.to_string()), Some(i64::from(r)))
        });
        let phase = phase.map(AlertPhase::as_str);
        let count = i64::from(limit) + 1;
        let rows=sqlx::query_as!(Row,r#"
          SELECT a.rule_id,a.revision,a.phase,t.phase AS transition_phase,a.version,a.candidate_since_ms,a.last_event_at_ms,
            COALESCE(h.active_revision=a.revision,false) AS "selected!",r.definition,
            t.event_at_ms,(floor(extract(epoch FROM t.recorded_at)*1000)::bigint) AS "recorded_at_ms!",
            p.activation_generation,t.evaluation_id,p.disposition,k.actor_id AS "ack_actor_id?",
            (floor(extract(epoch FROM k.recorded_at)*1000)::bigint) AS "ack_recorded_at_ms?"
          FROM analytics.alert_lifecycles a
          JOIN analytics.alert_rules h USING(tenant_id,rule_id)
          JOIN analytics.alert_rule_revisions r ON r.tenant_id=a.tenant_id AND r.rule_id=a.rule_id AND r.revision=a.revision
          JOIN analytics.alert_transitions t ON t.tenant_id=a.tenant_id AND t.rule_id=a.rule_id AND t.revision=a.revision AND t.version=a.version
          JOIN analytics.alert_evaluation_receipts p ON p.tenant_id=t.tenant_id AND p.rule_id=t.rule_id AND p.revision=t.revision AND p.evaluation_id=t.evaluation_id
          LEFT JOIN analytics.alert_acknowledgements k ON k.tenant_id=t.tenant_id AND k.rule_id=t.rule_id AND k.revision=t.revision AND k.version=t.version
          WHERE a.tenant_id=$1 AND a.version>0
            AND (($2::text IS NULL AND a.phase IN ('pending','firing')) OR a.phase=$2)
            AND ($3::text IS NULL OR (a.rule_id COLLATE "C",a.revision)>($3 COLLATE "C",$4))
          ORDER BY a.rule_id COLLATE "C",a.revision LIMIT $5
        "#,auth.tenant_id().as_str(),phase,id,revision,count).fetch_all(&self.0).await.map_err(AlertReadError::Database)?;
        rows.into_iter().map(convert).collect()
    }
    /// Reads an observed exact revision; unobserved/foreign/missing rows return none.
    /// # Errors
    /// Returns a bounded storage or invalid persisted-content failure.
    pub async fn find(
        &self,
        auth: &AuthorizedTenant<ReadAlerts>,
        rule: &AlertRuleId,
        revision: u32,
    ) -> Result<Option<AlertRead>, AlertReadError> {
        let revision = i64::from(revision);
        let row=sqlx::query_as!(Row,r#"
          SELECT a.rule_id,a.revision,a.phase,t.phase AS transition_phase,a.version,a.candidate_since_ms,a.last_event_at_ms,
            COALESCE(h.active_revision=a.revision,false) AS "selected!",r.definition,
            t.event_at_ms,(floor(extract(epoch FROM t.recorded_at)*1000)::bigint) AS "recorded_at_ms!",
            p.activation_generation,t.evaluation_id,p.disposition,k.actor_id AS "ack_actor_id?",
            (floor(extract(epoch FROM k.recorded_at)*1000)::bigint) AS "ack_recorded_at_ms?"
          FROM analytics.alert_lifecycles a
          JOIN analytics.alert_rules h USING(tenant_id,rule_id)
          JOIN analytics.alert_rule_revisions r ON r.tenant_id=a.tenant_id AND r.rule_id=a.rule_id AND r.revision=a.revision
          JOIN analytics.alert_transitions t ON t.tenant_id=a.tenant_id AND t.rule_id=a.rule_id AND t.revision=a.revision AND t.version=a.version
          JOIN analytics.alert_evaluation_receipts p ON p.tenant_id=t.tenant_id AND p.rule_id=t.rule_id AND p.revision=t.revision AND p.evaluation_id=t.evaluation_id
          LEFT JOIN analytics.alert_acknowledgements k ON k.tenant_id=t.tenant_id AND k.rule_id=t.rule_id AND k.revision=t.revision AND k.version=t.version
          WHERE a.tenant_id=$1 AND a.rule_id=$2 AND a.revision=$3 AND a.version>0
        "#,auth.tenant_id().as_str(),rule.as_str(),revision).fetch_optional(&self.0).await.map_err(AlertReadError::Database)?;
        row.map(convert).transpose()
    }
    /// Reads limit+1 immutable transitions strictly after a positive version.
    /// Caller separately distinguishes an unknown alert from an exhausted page.
    /// # Errors
    /// Rejects invalid limits/positions or malformed stored transition content.
    pub async fn transitions(
        &self,
        auth: &AuthorizedTenant<ReadAlerts>,
        rule: &AlertRuleId,
        revision: u32,
        limit: u16,
        after: Option<i64>,
    ) -> Result<Vec<AlertTransitionRead>, AlertReadError> {
        if !(1..=100).contains(&limit) || after.is_some_and(|v| v <= 0) {
            return Err(AlertReadError::Invalid);
        }
        let revision = i64::from(revision);
        let count = i64::from(limit) + 1;
        let after = after.unwrap_or(0);
        let rows=sqlx::query_as!(TransitionRow,r#"
          SELECT t.version,t.phase,t.event_at_ms,
            (floor(extract(epoch FROM t.recorded_at)*1000)::bigint) AS "recorded_at_ms!",
            p.activation_generation,t.evaluation_id,p.disposition,k.actor_id AS "ack_actor_id?",
            (floor(extract(epoch FROM k.recorded_at)*1000)::bigint) AS "ack_recorded_at_ms?"
          FROM analytics.alert_transitions t JOIN analytics.alert_evaluation_receipts p
            ON p.tenant_id=t.tenant_id AND p.rule_id=t.rule_id AND p.revision=t.revision AND p.evaluation_id=t.evaluation_id
          LEFT JOIN analytics.alert_acknowledgements k ON k.tenant_id=t.tenant_id AND k.rule_id=t.rule_id AND k.revision=t.revision AND k.version=t.version
          WHERE t.tenant_id=$1 AND t.rule_id=$2 AND t.revision=$3 AND t.version>$4
          ORDER BY t.version LIMIT $5
        "#,auth.tenant_id().as_str(),rule.as_str(),revision,after,count).fetch_all(&self.0).await.map_err(AlertReadError::Database)?;
        rows.into_iter().map(transition).collect()
    }
}
fn phase(s: &str) -> Result<AlertPhase, AlertReadError> {
    match s {
        "inactive" => Ok(AlertPhase::Inactive),
        "pending" => Ok(AlertPhase::Pending),
        "firing" => Ok(AlertPhase::Firing),
        "resolved" => Ok(AlertPhase::Resolved),
        _ => Err(AlertReadError::Invalid),
    }
}
fn transition(r: TransitionRow) -> Result<AlertTransitionRead, AlertReadError> {
    if r.version <= 0 {
        return Err(AlertReadError::Invalid);
    }
    let acknowledgement = match (r.ack_actor_id, r.ack_recorded_at_ms) {
        (None, None) => None,
        (Some(actor), Some(time)) => Some(AlertAcknowledgement {
            version: r.version,
            actor: ActorId::new(actor).map_err(|_| AlertReadError::Invalid)?,
            recorded_at: UnixTimeMillis::new(time),
        }),
        _ => return Err(AlertReadError::Invalid),
    };
    Ok(AlertTransitionRead {
        acknowledgement,
        version: r.version,
        phase: phase(&r.phase)?,
        event_at: UnixTimeMillis::new(r.event_at_ms),
        recorded_at: UnixTimeMillis::new(r.recorded_at_ms),
        activation_generation: u32::try_from(r.activation_generation)
            .map_err(|_| AlertReadError::Invalid)?,
        evaluation: CalculatorEvaluationId::new(r.evaluation_id)
            .map_err(|_| AlertReadError::Invalid)?,
        output_available: match r.disposition.as_str() {
            "applied" => true,
            "missing" => false,
            _ => return Err(AlertReadError::Invalid),
        },
    })
}
fn convert(r: Row) -> Result<AlertRead, AlertReadError> {
    let state = AlertLifecycle::restore(
        phase(&r.phase)?,
        r.candidate_since_ms.map(UnixTimeMillis::new),
        r.last_event_at_ms.map(UnixTimeMillis::new),
    )
    .ok_or(AlertReadError::Invalid)?;
    let definition =
        crate::alert_rules::decode(&r.definition).map_err(|_| AlertReadError::Invalid)?;
    let latest = transition(TransitionRow {
        version: r.version,
        phase: r.transition_phase,
        event_at_ms: r.event_at_ms,
        recorded_at_ms: r.recorded_at_ms,
        activation_generation: r.activation_generation,
        evaluation_id: r.evaluation_id,
        disposition: r.disposition,
        ack_actor_id: r.ack_actor_id,
        ack_recorded_at_ms: r.ack_recorded_at_ms,
    })?;
    if latest.phase != state.phase() || state.last_event().is_none_or(|last| last < latest.event_at)
    {
        return Err(AlertReadError::Invalid);
    }
    Ok(AlertRead {
        rule: AlertRuleId::new(r.rule_id).map_err(|_| AlertReadError::Invalid)?,
        revision: u32::try_from(r.revision).map_err(|_| AlertReadError::Invalid)?,
        state,
        version: r.version,
        selected: r.selected,
        definition,
        latest,
    })
}
/// Bounded read failure without arbitrary database or input diagnostics.
#[derive(Debug)]
pub enum AlertReadError {
    /// Invalid caller bounds or malformed retained typed values.
    Invalid,
    /// Unexpected storage failure; internal source is retained, never serialized.
    Database(sqlx::Error),
}
impl std::fmt::Display for AlertReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("alert read unavailable")
    }
}
impl std::error::Error for AlertReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(e) => Some(e),
            Self::Invalid => None,
        }
    }
}
