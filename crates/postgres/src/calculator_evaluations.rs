//! Atomic immutable envelopes; fixed SQL and full-content retry comparison.
use fleetiq_analytics::{
    CalculatorEvaluationRecord, CalculatorEvaluationStore, CalculatorExecutionFailure,
    CalculatorExecutionOutcome, EvaluationAppendOutcome,
};
use fleetiq_domain::PropertyValue;
use serde_json::{Value, json};
use sqlx::PgPool;

/// PostgreSQL terminal evaluation store; no scheduling or property projection.
pub struct PostgresCalculatorEvaluationStore(PgPool);
impl PostgresCalculatorEvaluationStore {
    /// Uses a pool with the complete migration chain already applied.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
}
impl CalculatorEvaluationStore for PostgresCalculatorEvaluationStore {
    type Error = EvaluationStoreError;
    async fn append(
        &self,
        record: &CalculatorEvaluationRecord,
    ) -> Result<EvaluationAppendOutcome, Self::Error> {
        let mut transaction = self.0.begin().await.map_err(|error| classify(&error))?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
            .execute(&mut *transaction)
            .await
            .map_err(|error| classify(&error))?;
        let outcome = append_record(&mut transaction, record).await?;
        transaction
            .commit()
            .await
            .map_err(|error| classify(&error))?;
        Ok(outcome)
    }
}
// Shared transaction boundary lets scheduled completion hold its current fence
// while appending outputs and checkpointing; ordinary one-shot semantics stay intact.
pub(crate) async fn append_record(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    record: &CalculatorEvaluationRecord,
) -> Result<EvaluationAppendOutcome, EvaluationStoreError> {
    let key_context = record.key();
    let manifest = key_context.manifest();
    let window = manifest.window();
    let inputs: Vec<_> = manifest
        .inputs()
        .iter()
        .map(|i| json!([i.id().as_str(), i.event_at().get()]))
        .collect();
    let key = json!({"scope":key_context.scope().as_str(),"calculator":key_context.calculator().id().as_str(),"version":key_context.calculator().version().as_str(),"configuration":key_context.configuration().as_str(),"start":window.start().get(),"end":window.end().get(),"finalize":window.finalize_at().get(),"known":manifest.known_at().get(),"inputs":inputs});
    let outputs:Vec<_>=record.outputs().iter().map(|o| {
            let property=o.property();
            let (kind,value)=match property.value() { PropertyValue::Decimal(v)=>("decimal",Value::String(v.normalized().to_string())),PropertyValue::Integer(v)=>("integer",json!(v)),PropertyValue::Boolean(v)=>("boolean",json!(v)),PropertyValue::Text(v)=>("text",json!(v)) };
            json!({"asset":property.asset_id().as_str(),"property":property.property().id().as_str(),"version":property.property().version().get(),"kind":kind,"value":value,"unit":property.unit().map(fleetiq_domain::UnitId::as_str)})
        }).collect();
    // Bound decimal/text representation before entering PostgreSQL's numeric parser.
    if outputs
        .iter()
        .any(|o| o["value"].as_str().is_some_and(|v| v.len() > 4096))
    {
        return Err(EvaluationStoreError::InvalidEvidence);
    }
    let outputs = Value::Array(outputs);
    let outcome = match record.outcome() {
        CalculatorExecutionOutcome::Complete => "complete",
        CalculatorExecutionOutcome::InsufficientEvidence => "insufficient",
        CalculatorExecutionOutcome::Failed(f) => match f {
            CalculatorExecutionFailure::IncompatibleInput => "incompatible_input",
            CalculatorExecutionFailure::InvalidOutput => "invalid_output",
            CalculatorExecutionFailure::ResourceLimit => "resource_limit",
        },
    };
    let tenant = record.evaluation().tenant_id().as_str();
    let id = record.evaluation().id().as_str();
    let evaluated = record.evaluation().evaluated_at().get();
    let minimum_samples = i32::from(record.minimum_samples());
    let inserted=sqlx::query_scalar!(r#"
            INSERT INTO analytics.evaluations (tenant_id,evaluation_id,replay_key,key_digest,evaluated_at_ms,outcome,outputs,minimum_samples)
            VALUES ($1,$2,$3,sha256(convert_to($3::jsonb::text,'UTF8')),$4,$5,$6,$7)
            ON CONFLICT DO NOTHING RETURNING TRUE AS "inserted!"
        "#,tenant,id,key,evaluated,outcome,outputs,minimum_samples).fetch_optional(&mut **transaction).await.map_err(|error| classify(&error))?;
    if inserted.is_some() {
        return Ok(EvaluationAppendOutcome::Appended);
    }
    // A new statement sees a concurrent committed winner at READ COMMITTED.
    let matches=sqlx::query_scalar!(r#"
            SELECT (replay_key=$3 AND evaluated_at_ms=$4 AND outcome=$5 AND outputs=$6 AND minimum_samples=$7) AS "matches!"
            FROM analytics.evaluations WHERE tenant_id=$1 AND evaluation_id=$2
        "#,tenant,id,key,evaluated,outcome,outputs,minimum_samples).fetch_optional(&mut **transaction).await.map_err(|error| classify(&error))?;
    if matches != Some(true) {
        return Err(EvaluationStoreError::IdentityCollision);
    }
    Ok(EvaluationAppendOutcome::AlreadyPresent)
}

/// Bounded append failures; no raw values or database diagnostics are exposed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvaluationStoreError {
    /// Identity or logical replay key was reused with different immutable content.
    IdentityCollision,
    /// Durable source/target or envelope validation failed.
    InvalidEvidence,
    /// Storage failed; retry the exact record after an ambiguous outcome.
    Database,
}
fn classify(error: &sqlx::Error) -> EvaluationStoreError {
    if error
        .as_database_error()
        .is_some_and(|db| db.code().as_deref() == Some("23505"))
    {
        return EvaluationStoreError::IdentityCollision;
    }
    if let sqlx::Error::Database(db) = error
        && matches!(
            db.code().as_deref(),
            Some("P0001" | "P0002" | "23503" | "23514" | "22P02" | "22003")
        )
    {
        return EvaluationStoreError::InvalidEvidence;
    }
    EvaluationStoreError::Database
}
impl std::fmt::Display for EvaluationStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "evaluation append failed: {self:?}")
    }
}
impl std::error::Error for EvaluationStoreError {}
