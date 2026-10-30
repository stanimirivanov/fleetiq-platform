//! Read-only terminal envelopes and ordered outputs, never fresh preparation.
use fleetiq_access::{AuthorizedTenant, ReadTelemetry};
use fleetiq_domain::AssetId;
use serde_json::Value;
use sqlx::PgPool;
/// Driver-independent persisted envelope projection; HTTP conversion is explicit.
pub struct CalculatorEvidenceRow {
    /// Immutable evaluation identity.
    pub id: String,
    /// First processing time, Unix milliseconds.
    pub evaluated_at_ms: i64,
    /// Retained terminal classification.
    pub outcome: String,
    /// Immutable minimum sample count.
    pub minimum_samples: i32,
    /// Replay header, optionally including bounded exact input references.
    pub replay: Value,
    /// Reviewed ABI of the immutable calculator release.
    pub implementation: String,
    /// Exact immutable input property definition.
    pub input_property_id: String,
    /// Input property version from the admitted release.
    pub input_property_version: i64,
    /// Number of retained input references.
    pub input_count: i32,
    /// Number of ordered terminal outputs.
    pub output_count: i32,
}
/// Persisted ordered output projection; no decoded payload expansion.
pub struct CalculatorOutputRow {
    /// Zero-based deterministic output position.
    pub index: i32,
    /// Exact stored property envelope, converted separately to the HTTP contract.
    pub output: Value,
}
/// Existing terminal storage read adapter; requires current telemetry authority.
pub struct PostgresCalculatorEvidence(PgPool);
impl PostgresCalculatorEvidence {
    /// Uses a checked-schema pool without migrations or writes.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    /// Reads at most limit+1 summaries in event/identity order.
    /// # Errors
    /// Rejects invalid bounds/limits and returns bounded storage failures.
    pub async fn list(
        &self,
        auth: &AuthorizedTenant<ReadTelemetry>,
        asset: &AssetId,
        start: i64,
        end: i64,
        limit: u16,
        after: Option<(i64, String)>,
    ) -> Result<Vec<CalculatorEvidenceRow>, CalculatorEvidenceError> {
        if !(1..=100).contains(&limit)
            || !(1..=604_800_000).contains(&(i128::from(end) - i128::from(start)))
        {
            return Err(CalculatorEvidenceError);
        }
        let (time, id) = after.map_or((None, None), |(t, id)| (Some(t), Some(id)));
        let count = i64::from(limit) + 1;
        sqlx::query_as!(CalculatorEvidenceRow,r#"
          SELECT e.evaluation_id AS id,e.evaluated_at_ms,e.outcome,e.minimum_samples,
            e.replay_key-'inputs' AS "replay!",d.implementation,d.input_id AS input_property_id,d.input_version AS input_property_version,
            jsonb_array_length(e.replay_key->'inputs') AS "input_count!",jsonb_array_length(e.outputs) AS "output_count!"
          FROM analytics.evaluations e JOIN analytics.calculator_definitions d USING(tenant_id,calculator_id,calculator_version)
          WHERE e.tenant_id=$1 AND e.scope_asset_id=$2 AND (e.replay_key->>'end')::bigint >= $3 AND (e.replay_key->>'end')::bigint < $4
            AND ($5::bigint IS NULL OR ((e.replay_key->>'end')::bigint,e.evaluation_id COLLATE "C")>($5,$6 COLLATE "C"))
          ORDER BY (e.replay_key->>'end')::bigint,e.evaluation_id COLLATE "C" LIMIT $7
        "#,auth.tenant_id().as_str(),asset.as_str(),start,end,time,id,count).fetch_all(&self.0).await.map_err(|_|CalculatorEvidenceError)
    }
    /// Reads one exact envelope; summaries can omit its input array.
    /// # Errors
    /// Returns a bounded storage error; foreign/missing records return none.
    pub async fn find(
        &self,
        auth: &AuthorizedTenant<ReadTelemetry>,
        asset: &AssetId,
        id: &str,
        inputs: bool,
    ) -> Result<Option<CalculatorEvidenceRow>, CalculatorEvidenceError> {
        sqlx::query_as!(CalculatorEvidenceRow,r#"
          SELECT e.evaluation_id AS id,e.evaluated_at_ms,e.outcome,e.minimum_samples,
            CASE WHEN $4 THEN e.replay_key ELSE e.replay_key-'inputs' END AS "replay!",d.implementation,d.input_id AS input_property_id,d.input_version AS input_property_version,
            jsonb_array_length(e.replay_key->'inputs') AS "input_count!",jsonb_array_length(e.outputs) AS "output_count!"
          FROM analytics.evaluations e JOIN analytics.calculator_definitions d USING(tenant_id,calculator_id,calculator_version)
          WHERE e.tenant_id=$1 AND e.scope_asset_id=$2 AND e.evaluation_id=$3
        "#,auth.tenant_id().as_str(),asset.as_str(),id,inputs).fetch_optional(&self.0).await.map_err(|_|CalculatorEvidenceError)
    }
    /// Reads limit+1 ordered outputs from one exact immutable evaluation.
    /// # Errors
    /// Rejects invalid limits/positions or bounded storage failure.
    pub async fn outputs(
        &self,
        auth: &AuthorizedTenant<ReadTelemetry>,
        asset: &AssetId,
        id: &str,
        limit: u16,
        after: Option<u32>,
    ) -> Result<Vec<CalculatorOutputRow>, CalculatorEvidenceError> {
        if !(1..=20).contains(&limit) || after.is_some_and(|i| i >= 1024) {
            return Err(CalculatorEvidenceError);
        }
        let index = after.map_or(-1, i64::from);
        let count = i64::from(limit) + 1;
        sqlx::query_as!(CalculatorOutputRow,r#"
          SELECT (o.ordinality-1)::int AS "index!",o.value AS "output!"
          FROM analytics.evaluations e CROSS JOIN LATERAL jsonb_array_elements(e.outputs) WITH ORDINALITY o
          WHERE e.tenant_id=$1 AND e.scope_asset_id=$2 AND e.evaluation_id=$3 AND o.ordinality>$4::bigint+1
          ORDER BY o.ordinality LIMIT $5
        "#,auth.tenant_id().as_str(),asset.as_str(),id,index,count).fetch_all(&self.0).await.map_err(|_|CalculatorEvidenceError)
    }
}
/// Bounded evidence-read failure without database diagnostics.
#[derive(Debug)]
pub struct CalculatorEvidenceError;
impl std::fmt::Display for CalculatorEvidenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("calculator evidence unavailable")
    }
}
impl std::error::Error for CalculatorEvidenceError {}
