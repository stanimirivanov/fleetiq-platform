//! Immutable operator intent, fenced against current evidence and authority.
use fleetiq_access::{AcknowledgeAlerts, AuthorizedTenant};
use fleetiq_analytics::AlertRuleId;
use fleetiq_domain::{ActorId, UnixTimeMillis};
use sqlx::PgPool;
/// Original receipt for acknowledging one existing phase-transition version.
pub struct AlertAcknowledgement {
    /// Existing lifecycle version; acknowledgement never increments it.
    pub version: i64,
    /// Original authenticated author.
    pub actor: ActorId,
    /// Original database receipt instant.
    pub recorded_at: UnixTimeMillis,
}
/// Typed current-authority acknowledgement adapter; no rule or notification writes.
pub struct PostgresAlertAcknowledgement(PgPool);
impl PostgresAlertAcknowledgement {
    /// Uses a checked-schema pool.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    /// Acknowledges the expected open version, or returns its original same-author receipt.
    /// The checkpoint lock serializes with evidence completion. Current authority is
    /// rechecked even for an old retry; no phase, timer, version or intent is changed.
    /// # Errors
    /// Rejects stale/closed/unobserved targets, another author and revoked authority.
    pub async fn acknowledge(
        &self,
        auth: AuthorizedTenant<AcknowledgeAlerts>,
        rule: &AlertRuleId,
        revision: u32,
        version: i64,
    ) -> Result<AlertAcknowledgement, AlertAcknowledgementError> {
        if revision == 0 || version <= 0 {
            return Err(AlertAcknowledgementError::Conflict);
        }
        let revision = i64::from(revision);
        let mut tx = self.0.begin().await.map_err(classify)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        sqlx::query!(
            "SELECT analytics.acknowledge_alert($1,$2,$3,$4,$5)",
            auth.tenant_id().as_str(),
            rule.as_str(),
            revision,
            version,
            auth.actor_id().as_str()
        )
        .execute(&mut *tx)
        .await
        .map_err(classify)?;
        let row=sqlx::query!(r#"
            SELECT actor_id,(floor(extract(epoch FROM recorded_at)*1000)::bigint) AS "recorded_at_ms!"
            FROM analytics.alert_acknowledgements WHERE tenant_id=$1 AND rule_id=$2 AND revision=$3 AND version=$4
        "#,auth.tenant_id().as_str(),rule.as_str(),revision,version).fetch_one(&mut *tx).await.map_err(classify)?;
        let receipt = AlertAcknowledgement {
            version,
            actor: ActorId::new(row.actor_id)
                .map_err(|_| AlertAcknowledgementError::InvalidReceipt)?,
            recorded_at: UnixTimeMillis::new(row.recorded_at_ms),
        };
        tx.commit().await.map_err(classify)?;
        Ok(receipt)
    }
}
/// Bounded acknowledgement outcomes, with internal storage causes retained.
#[derive(Debug)]
pub enum AlertAcknowledgementError {
    /// Current grant, actor or tenant lifecycle denies the command/retry.
    Forbidden,
    /// Stale/closed/unobserved identity or a different original author.
    Conflict,
    /// Malformed immutable receipt cannot be reconstructed safely.
    InvalidReceipt,
    /// Storage failed and the exact command may be retried.
    Database(sqlx::Error),
}
fn classify(e: sqlx::Error) -> AlertAcknowledgementError {
    match e
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .as_deref()
    {
        Some("42501") => AlertAcknowledgementError::Forbidden,
        Some("23505" | "23503" | "23514") => AlertAcknowledgementError::Conflict,
        _ => AlertAcknowledgementError::Database(e),
    }
}
impl std::fmt::Display for AlertAcknowledgementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("alert acknowledgement rejected")
    }
}
impl std::error::Error for AlertAcknowledgementError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(e) => Some(e),
            _ => None,
        }
    }
}
