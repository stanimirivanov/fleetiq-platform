//! Authorized immutable alert rules and optimistic activation receipts.
use fleetiq_access::{AuthorizedTenant, ManageAlerts, ReadAlerts};
use fleetiq_analytics::{
    ALERT_RULE_GRAMMAR, AlertComparison, AlertResolution, AlertRuleDefinition, AlertRuleId,
    CalculatorId, CalculatorRef, CalculatorVersion,
};
use fleetiq_domain::{
    AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, TenantId, UnitId,
};
use serde_json::{Value, json};
use sqlx::PgPool;
/// Exact retained definition and original command receipt.
pub struct AlertRuleRevision {
    /// Immutable rule parameters.
    pub definition: AlertRuleDefinition,
    /// Positive rule revision.
    pub revision: u32,
    /// Original authenticated author.
    pub actor: String,
    /// Original database-clock Unix millisecond receipt time.
    pub recorded_at: i64,
}
/// Current authoring head and activation selection.
pub struct AlertRuleHead {
    /// Latest retained definition revision.
    pub latest_revision: u32,
    /// Current activation receipt, independent of definition creation.
    pub activation: AlertRuleActivationRecord,
}
/// Current or original retried activation receipt.
pub struct AlertRuleActivationRecord {
    /// Optimistic state generation; zero is initial inactive state.
    pub generation: u32,
    /// Selected immutable revision, or explicit deactivation.
    pub revision: Option<u32>,
    /// Original command author; absent at initial inactive state.
    pub actor: Option<String>,
    /// Original receipt time, absent at initial inactive state.
    pub recorded_at: Option<i64>,
}
/// PostgreSQL adapter; authorization evidence is consumed at the boundary.
pub struct PostgresAlertRules(PgPool);
impl PostgresAlertRules {
    /// Uses the caller's checked-schema pool.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    /// Creates one next revision or accepts an identical same-author retry.
    /// # Errors
    /// Rejects stale revision, changed content, incompatible references or revoked authority.
    pub async fn create(
        &self,
        auth: AuthorizedTenant<ManageAlerts>,
        id: &AlertRuleId,
        revision: u32,
        definition: &AlertRuleDefinition,
    ) -> Result<(), AlertRuleError> {
        if !definition.is_valid() {
            return Err(AlertRuleError::Conflict);
        }
        let p = definition_json(definition);
        let revision = i64::from(revision);
        sqlx::query!(
            "SELECT analytics.create_alert_rule_revision($1,$2,$3,$4,$5)",
            auth.tenant_id().as_str(),
            id.as_str(),
            revision,
            auth.actor_id().as_str(),
            p
        )
        .execute(&self.0)
        .await
        .map_err(|error| classify(&error))?;
        Ok(())
    }
    /// Reads exact retained content under current alerts-read authority.
    /// # Errors
    /// Fails closed on storage errors or malformed persisted values.
    pub async fn find(
        &self,
        auth: AuthorizedTenant<ReadAlerts>,
        id: &AlertRuleId,
        revision: u32,
    ) -> Result<Option<AlertRuleRevision>, AlertRuleError> {
        let revision = i64::from(revision);
        let row = sqlx::query!(
            "SELECT definition,actor_id,recorded_at_ms FROM analytics.alert_rule_revisions WHERE tenant_id=$1 AND rule_id=$2 AND revision=$3",
            auth.tenant_id().as_str(),
            id.as_str(),
            revision
        )
        .fetch_optional(&self.0)
        .await
        .map_err(|e| classify(&e))?;
        row.map(|r| {
            Ok(AlertRuleRevision {
                definition: decode(&r.definition)?,
                revision: u32::try_from(revision).map_err(|_| AlertRuleError::Database)?,
                actor: r.actor_id,
                recorded_at: r.recorded_at_ms,
            })
        })
        .transpose()
    }
    /// Reads current head and selection; missing rules return none.
    /// # Errors
    /// Returns a bounded storage or persisted-value failure.
    pub async fn head(
        &self,
        auth: AuthorizedTenant<ReadAlerts>,
        id: &AlertRuleId,
    ) -> Result<Option<AlertRuleHead>, AlertRuleError> {
        let row = sqlx::query!(
            "SELECT h.latest_revision,h.generation,a.revision,a.actor_id,a.recorded_at_ms FROM analytics.alert_rules h LEFT JOIN analytics.alert_rule_activation_changes a ON a.tenant_id=h.tenant_id AND a.rule_id=h.rule_id AND a.generation=h.generation WHERE h.tenant_id=$1 AND h.rule_id=$2",
            auth.tenant_id().as_str(),
            id.as_str()
        )
        .fetch_optional(&self.0)
        .await
        .map_err(|e| classify(&e))?;
        row.map(|r| {
            Ok(AlertRuleHead {
                latest_revision: u32::try_from(r.latest_revision)
                    .map_err(|_| AlertRuleError::Database)?,
                activation: AlertRuleActivationRecord {
                    generation: u32::try_from(r.generation)
                        .map_err(|_| AlertRuleError::Database)?,
                    revision: r
                        .revision
                        .map(u32::try_from)
                        .transpose()
                        .map_err(|_| AlertRuleError::Database)?,
                    actor: r.actor_id,
                    recorded_at: r.recorded_at_ms,
                },
            })
        })
        .transpose()
    }
    /// Changes selection with exact same-author retry and transactional authorization.
    /// # Errors
    /// Rejects revoked authority, unavailable revisions and conflicting generations.
    pub async fn activate(
        &self,
        auth: AuthorizedTenant<ManageAlerts>,
        id: &AlertRuleId,
        expected: u32,
        revision: Option<u32>,
    ) -> Result<AlertRuleActivationRecord, AlertRuleError> {
        let expected = i64::from(expected);
        let revision = revision.map(i64::from);
        let generation = sqlx::query_scalar!(
            r#"SELECT analytics.change_alert_rule_activation($1,$2,$3,$4,$5) AS "generation!""#,
            auth.tenant_id().as_str(),
            id.as_str(),
            expected,
            revision,
            auth.actor_id().as_str()
        )
        .fetch_one(&self.0)
        .await
        .map_err(|error| classify(&error))?;
        self.receipt(auth.tenant_id(), id, Some(generation))
            .await?
            .ok_or(AlertRuleError::Database)
    }
    async fn receipt(
        &self,
        tenant: &TenantId,
        id: &AlertRuleId,
        generation: Option<i64>,
    ) -> Result<Option<AlertRuleActivationRecord>, AlertRuleError> {
        let row = sqlx::query!(
            "SELECT COALESCE($3,c.generation) AS generation,r.revision,r.actor_id,r.recorded_at_ms FROM analytics.alert_rules c LEFT JOIN analytics.alert_rule_activation_changes r ON r.tenant_id=c.tenant_id AND r.rule_id=c.rule_id AND r.generation=COALESCE($3,c.generation) WHERE c.tenant_id=$1 AND c.rule_id=$2",
            tenant.as_str(),
            id.as_str(),
            generation
        )
        .fetch_optional(&self.0)
        .await
        .map_err(|e| classify(&e))?;
        row.map(|r| {
            Ok(AlertRuleActivationRecord {
                generation: u32::try_from(r.generation.ok_or(AlertRuleError::Database)?)
                    .map_err(|_| AlertRuleError::Database)?,
                revision: r
                    .revision
                    .map(u32::try_from)
                    .transpose()
                    .map_err(|_| AlertRuleError::Database)?,
                actor: r.actor_id,
                recorded_at: r.recorded_at_ms,
            })
        })
        .transpose()
    }
}

fn definition_json(d: &AlertRuleDefinition) -> Value {
    json!({
        "kind": ALERT_RULE_GRAMMAR,
        "calculator": d.calculator.id().as_str(),
        "version": d.calculator.version().as_str(),
        "scope": d.scope.as_str(),
        "target": d.target.as_str(),
        "output_property": d.output.id().as_str(),
        "output_version": d.output.version().get(),
        "output_index": d.output_index,
        "comparison": d.comparison.as_str(),
        "threshold": d.threshold.normalized().to_string(),
        "threshold_unit": d.unit.as_ref().map(UnitId::as_str),
        "hold_ms": d.hold_ms,
        "resolution": "resolve_on_false",
        "resolve_after_ms": d.resolve_after_ms,
    })
}
fn decode(p: &Value) -> Result<AlertRuleDefinition, AlertRuleError> {
    let parse = || -> Result<AlertRuleDefinition, Box<dyn std::error::Error>> {
        let text = |k| p[k].as_str().ok_or(AlertRuleError::Database);
        let number = |k| p[k].as_u64().ok_or(AlertRuleError::Database);
        if text("kind")? != ALERT_RULE_GRAMMAR
            || text("resolution")? != "resolve_on_false"
            || text("threshold")?.len() > 256
        {
            return Err(AlertRuleError::Database.into());
        }
        let d = AlertRuleDefinition {
            calculator: CalculatorRef::new(
                CalculatorId::new(text("calculator")?)?,
                CalculatorVersion::new(text("version")?)?,
            ),
            scope: AssetId::new(text("scope")?)?,
            target: AssetId::new(text("target")?)?,
            output: PropertyDefinitionRef::new(
                PropertyDefinitionId::new(text("output_property")?)?,
                DefinitionVersion::new(u32::try_from(number("output_version")?)?)?,
            ),
            output_index: u32::try_from(number("output_index")?)?,
            comparison: AlertComparison::parse(text("comparison")?)
                .ok_or(AlertRuleError::Database)?,
            threshold: text("threshold")?.parse()?,
            unit: match &p["threshold_unit"] {
                Value::Null => None,
                Value::String(v) => Some(UnitId::new(v)?),
                _ => return Err(AlertRuleError::Database.into()),
            },
            hold_ms: u32::try_from(number("hold_ms")?)?,
            resolution: AlertResolution::OnFalse,
            resolve_after_ms: u32::try_from(number("resolve_after_ms")?)?,
        };
        if !d.is_valid() {
            return Err(AlertRuleError::Database.into());
        }
        Ok(d)
    };
    parse().map_err(|_| AlertRuleError::Database)
}
/// Bounded rule-storage failure; no database diagnostics cross the boundary.
#[derive(Clone, Copy, Debug)]
pub enum AlertRuleError {
    /// Current authority or lifecycle state rejects the mutation.
    Forbidden,
    /// Stale revision/generation, changed retry or incompatible selector.
    Conflict,
    /// Storage failure or corrupt persisted definition.
    Database,
}
fn classify(e: &sqlx::Error) -> AlertRuleError {
    match e
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .as_deref()
    {
        Some("42501") => AlertRuleError::Forbidden,
        Some("23505" | "23503" | "23514") => AlertRuleError::Conflict,
        _ => AlertRuleError::Database,
    }
}
impl std::fmt::Display for AlertRuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "alert rule configuration failed: {self:?}")
    }
}
impl std::error::Error for AlertRuleError {}
