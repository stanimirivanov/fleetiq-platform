//! Immutable first-party calculator catalogue; no activation or execution.
use fleetiq_analytics::{CalculatorDefinition, CalculatorRegistrationOutcome};
use sqlx::PgPool;

/// PostgreSQL registry scoped to an existing, trusted database connection.
pub struct PostgresCalculatorRegistry(PgPool);
impl PostgresCalculatorRegistry {
    /// Uses the caller's pool; does not migrate or alter runtime privileges.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    /// Registers exact content or resolves a concurrent retry in a fresh snapshot.
    /// # Errors
    /// Returns a bounded error for invalid/missing references, content collision
    /// or database failure. Property definitions are immutable; SELECT validates
    /// numeric input, decimal output and matching units before insertion.
    pub async fn register(
        &self,
        d: &CalculatorDefinition,
    ) -> Result<CalculatorRegistrationOutcome, CalculatorRegistryError> {
        let tenant = d.tenant.as_str();
        let id = d.release.id().as_str();
        let version = d.release.version().as_str();
        let implementation = d.implementation.as_str();
        let input = d.input.id().as_str();
        let iv = i64::from(d.input.version().get());
        let output = d.output.id().as_str();
        let ov = i64::from(d.output.version().get());
        let inserted=sqlx::query_scalar!(r#"
            INSERT INTO analytics.calculator_definitions
              (tenant_id, calculator_id, calculator_version, implementation, input_id, input_version, output_id, output_version)
            SELECT $1, $2, $3, $4, $5, $6, $7, $8
            FROM metadata.property_definitions i JOIN metadata.property_definitions o ON o.tenant_id = i.tenant_id
            WHERE i.tenant_id = $1 AND i.property_id = $5 AND i.version = $6
              AND o.property_id = $7 AND o.version = $8
              AND i.value_kind IN ('integer', 'decimal') AND o.value_kind = 'decimal'
              AND i.canonical_unit IS NOT DISTINCT FROM o.canonical_unit
            ON CONFLICT DO NOTHING RETURNING TRUE AS "inserted!"
        "#,tenant,id,version,implementation,input,iv,output,ov).fetch_optional(&self.0).await.map_err(|_|CalculatorRegistryError::Database)?;
        if inserted.is_some() {
            return Ok(CalculatorRegistrationOutcome::Registered);
        }
        let matches = sqlx::query_scalar!(
            r#"
            SELECT (implementation = $4 AND input_id = $5 AND input_version = $6
              AND output_id = $7 AND output_version = $8) AS "matches!"
            FROM analytics.calculator_definitions
            WHERE tenant_id = $1 AND calculator_id = $2 AND calculator_version = $3
        "#,
            tenant,
            id,
            version,
            implementation,
            input,
            iv,
            output,
            ov
        )
        .fetch_optional(&self.0)
        .await
        .map_err(|_| CalculatorRegistryError::Database)?;
        match matches {
            Some(true) => Ok(CalculatorRegistrationOutcome::AlreadyPresent),
            Some(false) => Err(CalculatorRegistryError::IdentityCollision),
            None => Err(CalculatorRegistryError::InvalidReferences),
        }
    }
}
/// Bounded registry failures; raw SQL diagnostics stay outside process output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculatorRegistryError {
    /// Existing tenant/release identity names different immutable content.
    IdentityCollision,
    /// Exact properties are missing, foreign, nonnumeric or unit-incompatible.
    InvalidReferences,
    /// The database operation failed; retry may be safe after an ambiguous commit.
    Database,
}
impl std::fmt::Display for CalculatorRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "calculator registration failed: {self:?}")
    }
}
impl std::error::Error for CalculatorRegistryError {}

impl fleetiq_analytics::CalculatorDefinitionRegistry for PostgresCalculatorRegistry {
    type Error = CalculatorRegistryError;
    async fn register(
        &self,
        definition: &CalculatorDefinition,
    ) -> Result<CalculatorRegistrationOutcome, Self::Error> {
        self.register(definition).await
    }
}
