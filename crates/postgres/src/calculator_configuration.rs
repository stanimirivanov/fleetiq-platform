//! Authorized immutable calculator revisions and optimistic activation receipts.
use fleetiq_access::{AuthorizedTenant, ManageAnalytics, ReadMetadata};
use fleetiq_analytics::{
    CalculatorConfigurationId, CalculatorId, CalculatorRef, CalculatorSchedule, CalculatorVersion,
};
use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use serde_json::{Value, json};
use sqlx::PgPool;

/// Retained configuration revision, including unknown legacy import attribution.
pub struct CalculatorConfigurationRecord {
    /// Exact immutable execution schedule.
    pub schedule: CalculatorSchedule,
    /// Configuration revision, independent of activation generation.
    pub revision: u32,
    /// Original author; absent only for imported trusted schedules.
    pub actor: Option<String>,
    /// Original receipt time, Unix milliseconds.
    pub recorded_at: i64,
}
/// Current or original retried activation receipt.
pub struct CalculatorActivationRecord {
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
pub struct PostgresCalculatorConfiguration(PgPool);
impl PostgresCalculatorConfiguration {
    /// Uses the caller's checked-schema pool.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    /// Creates one next revision or accepts an identical same-author retry.
    /// # Errors
    /// Rejects stale revision, changed content, incompatible references or revoked authority.
    pub async fn create(
        &self,
        auth: AuthorizedTenant<ManageAnalytics>,
        id: &CalculatorConfigurationId,
        revision: u32,
        schedule: &CalculatorSchedule,
    ) -> Result<(), CalculatorConfigurationError> {
        if !schedule.is_valid() || schedule.tenant != *auth.tenant_id() {
            return Err(CalculatorConfigurationError::Conflict);
        }
        let p = parameters(schedule);
        let revision = i64::from(revision);
        sqlx::query!(
            "SELECT analytics.create_configuration_revision($1,$2,$3,$4,$5)",
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
    /// Reads exact retained content under current metadata-read authority.
    /// # Errors
    /// Fails closed on storage errors or malformed persisted values.
    pub async fn find(
        &self,
        auth: AuthorizedTenant<ReadMetadata>,
        id: &CalculatorConfigurationId,
        revision: u32,
    ) -> Result<Option<CalculatorConfigurationRecord>, CalculatorConfigurationError> {
        let revision = i64::from(revision);
        let row=sqlx::query!("SELECT r.parameters,r.actor_id,r.recorded_at_ms,s.schedule_id,s.configuration_id AS execution_id FROM analytics.calculator_configuration_revisions r JOIN analytics.calculator_schedules s ON s.tenant_id=r.tenant_id AND s.schedule_id=r.schedule_id WHERE r.tenant_id=$1 AND r.configuration_id=$2 AND r.revision=$3",auth.tenant_id().as_str(),id.as_str(),revision).fetch_optional(&self.0).await.map_err(|error| classify(&error))?;
        row.map(|r| {
            Ok(CalculatorConfigurationRecord {
                schedule: decode(
                    auth.tenant_id(),
                    &r.schedule_id,
                    &r.execution_id,
                    &r.parameters,
                )?,
                revision: u32::try_from(revision)
                    .map_err(|_| CalculatorConfigurationError::Database)?,
                actor: r.actor_id,
                recorded_at: r.recorded_at_ms,
            })
        })
        .transpose()
    }
    /// Reads current activation, returning none for an undiscoverable configuration.
    /// # Errors
    /// Returns a bounded storage failure.
    pub async fn activation(
        &self,
        auth: AuthorizedTenant<ReadMetadata>,
        id: &CalculatorConfigurationId,
    ) -> Result<Option<CalculatorActivationRecord>, CalculatorConfigurationError> {
        self.receipt(auth.tenant_id(), id, None).await
    }
    /// Changes selection with exact same-author retry and transactional authorization.
    /// # Errors
    /// Rejects revoked authority, unavailable revisions and conflicting generations.
    pub async fn activate(
        &self,
        auth: AuthorizedTenant<ManageAnalytics>,
        id: &CalculatorConfigurationId,
        expected: u32,
        revision: Option<u32>,
    ) -> Result<CalculatorActivationRecord, CalculatorConfigurationError> {
        let expected = i64::from(expected);
        let revision = revision.map(i64::from);
        let generation = sqlx::query_scalar!(
            r#"SELECT analytics.change_configuration_activation($1,$2,$3,$4,$5) AS "generation!""#,
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
            .ok_or(CalculatorConfigurationError::Database)
    }
    async fn receipt(
        &self,
        tenant: &TenantId,
        id: &CalculatorConfigurationId,
        generation: Option<i64>,
    ) -> Result<Option<CalculatorActivationRecord>, CalculatorConfigurationError> {
        let row=sqlx::query!("SELECT COALESCE($3,c.generation) AS generation,r.revision,r.actor_id,r.recorded_at_ms FROM analytics.calculator_configurations c LEFT JOIN analytics.calculator_activation_changes r ON r.tenant_id=c.tenant_id AND r.configuration_id=c.configuration_id AND r.generation=COALESCE($3,c.generation) WHERE c.tenant_id=$1 AND c.configuration_id=$2",tenant.as_str(),id.as_str(),generation).fetch_optional(&self.0).await.map_err(|error| classify(&error))?;
        row.map(|r| {
            Ok(CalculatorActivationRecord {
                generation: u32::try_from(
                    r.generation.ok_or(CalculatorConfigurationError::Database)?,
                )
                .map_err(|_| CalculatorConfigurationError::Database)?,
                revision: r
                    .revision
                    .map(u32::try_from)
                    .transpose()
                    .map_err(|_| CalculatorConfigurationError::Database)?,
                actor: r.actor_id,
                recorded_at: r.recorded_at_ms,
            })
        })
        .transpose()
    }
}
fn parameters(s: &CalculatorSchedule) -> Value {
    json!({"calculator":s.calculator.id().as_str(),"version":s.calculator.version().as_str(),"scope":s.scope.as_str(),"target":s.target.as_str(),"first_start":s.first_start.get(),"window_ms":s.window_ms,"lateness_ms":s.lateness_ms,"minimum_samples":s.minimum_samples})
}
fn decode(
    tenant: &TenantId,
    id: &str,
    execution: &str,
    p: &Value,
) -> Result<CalculatorSchedule, CalculatorConfigurationError> {
    let parse = || -> Result<CalculatorSchedule, Box<dyn std::error::Error>> {
        let text = |k| p[k].as_str().ok_or(CalculatorConfigurationError::Database);
        let number = |k| p[k].as_u64().ok_or(CalculatorConfigurationError::Database);
        Ok(CalculatorSchedule {
            tenant: tenant.clone(),
            id: CalculatorConfigurationId::new(id)?,
            calculator: CalculatorRef::new(
                CalculatorId::new(text("calculator")?)?,
                CalculatorVersion::new(text("version")?)?,
            ),
            configuration: CalculatorConfigurationId::new(execution)?,
            scope: AssetId::new(text("scope")?)?,
            target: AssetId::new(text("target")?)?,
            first_start: UnixTimeMillis::new(
                p["first_start"]
                    .as_i64()
                    .ok_or(CalculatorConfigurationError::Database)?,
            ),
            window_ms: u32::try_from(number("window_ms")?)?,
            lateness_ms: u32::try_from(number("lateness_ms")?)?,
            minimum_samples: u16::try_from(number("minimum_samples")?)?,
        })
    };
    let result = parse().map_err(|_| CalculatorConfigurationError::Database)?;
    if !result.is_valid() {
        return Err(CalculatorConfigurationError::Database);
    }
    Ok(result)
}
/// Bounded errors; no SQL source or configuration content crosses HTTP.
#[derive(Clone, Copy, Debug)]
pub enum CalculatorConfigurationError {
    /// Authority or actor/tenant lifecycle no longer permits mutation.
    Forbidden,
    /// Revision/generation conflict or unavailable compatible references.
    Conflict,
    /// Storage failure or corrupt persisted content.
    Database,
}
fn classify(e: &sqlx::Error) -> CalculatorConfigurationError {
    match e
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .as_deref()
    {
        Some("42501") => CalculatorConfigurationError::Forbidden,
        Some("23505" | "23503" | "23514") => CalculatorConfigurationError::Conflict,
        _ => CalculatorConfigurationError::Database,
    }
}
impl std::fmt::Display for CalculatorConfigurationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "calculator configuration failed: {self:?}")
    }
}
impl std::error::Error for CalculatorConfigurationError {}
