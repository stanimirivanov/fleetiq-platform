//! Database-clock scheduling, claims and atomic generation-fenced completion.
use crate::{calculator_evaluations::append_record, calculator_inputs::request_json};
use fleetiq_analytics::{
    CalculatorConfigurationId, CalculatorEvaluation, CalculatorEvaluationId,
    CalculatorEvaluationRecord, CalculatorId, CalculatorRef, CalculatorRunRequest,
    CalculatorSchedule, CalculatorVersion, CalculatorWindow, CalculatorWorkLease,
};
use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use serde_json::{Value, json};
use sqlx::PgPool;

/// Trusted fixed-window schedule registry and durable work adapter.
pub struct PostgresCalculatorWork(PgPool);
impl PostgresCalculatorWork {
    /// Uses a checked-schema pool; workers must set bounded statement deadlines.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    /// Installs immutable trusted schedule content, accepting an exact retry.
    /// # Errors
    /// Rejects invalid bounds, target membership, foreign references or changed IDs.
    pub async fn register(&self, s: &CalculatorSchedule) -> Result<(), CalculatorWorkError> {
        if !s.is_valid() {
            return Err(CalculatorWorkError);
        }
        let tenant = s.tenant.as_str();
        let id = s.id.as_str();
        let calculator = s.calculator.id().as_str();
        let version = s.calculator.version().as_str();
        let configuration = s.configuration.as_str();
        let scope = s.scope.as_str();
        let target = s.target.as_str();
        let start = s.first_start.get();
        let window = i64::from(s.window_ms);
        let lateness = i64::from(s.lateness_ms);
        let minimum = i32::from(s.minimum_samples);
        let inserted=sqlx::query_scalar!(r#"
            INSERT INTO analytics.calculator_schedules
              (tenant_id,schedule_id,calculator_id,calculator_version,configuration_id,scope_asset_id,target_asset_id,first_start_ms,window_ms,lateness_ms,minimum_samples,next_start_ms)
            SELECT $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$8 FROM analytics.calculator_definitions d
            JOIN metadata.assets a ON a.tenant_id=d.tenant_id AND a.asset_id=$7
            JOIN metadata.asset_type_properties p ON p.tenant_id=a.tenant_id AND p.type_id=a.type_id AND p.type_version=a.type_version AND p.property_id=d.output_id AND p.property_version=d.output_version
            WHERE d.tenant_id=$1 AND d.calculator_id=$3 AND d.calculator_version=$4
              AND EXISTS(SELECT 1 FROM metadata.assets WHERE tenant_id=$1 AND asset_id=$6)
            ON CONFLICT DO NOTHING RETURNING TRUE AS "inserted!"
        "#,tenant,id,calculator,version,configuration,scope,target,start,window,lateness,minimum).fetch_optional(&self.0).await.map_err(|_|CalculatorWorkError)?;
        if inserted.is_some() {
            return Ok(());
        }
        let expected = json!({"tenant_id":tenant,"schedule_id":id,"calculator_id":calculator,"calculator_version":version,"configuration_id":configuration,"scope_asset_id":scope,"target_asset_id":target,"first_start_ms":start,"window_ms":window,"lateness_ms":lateness,"minimum_samples":minimum});
        let matches=sqlx::query_scalar!(r#"SELECT (to_jsonb(s)-'next_start_ms'=$3) AS "matches!" FROM analytics.calculator_schedules s WHERE tenant_id=$1 AND schedule_id=$2"#,tenant,id,expected).fetch_optional(&self.0).await.map_err(|_|CalculatorWorkError)?;
        if matches == Some(true) {
            Ok(())
        } else {
            Err(CalculatorWorkError)
        }
    }
    /// Enumerates at most 32 finalized windows for one locked active schedule.
    /// # Errors
    /// Storage failure rolls back both inserted work and cursor advancement.
    pub async fn seed(&self) -> Result<i64, CalculatorWorkError> {
        sqlx::query_scalar!(r#"
            WITH clock AS MATERIALIZED (SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint AS now),
            candidate AS MATERIALIZED (
              SELECT s.*,clock.now,LEAST(32::numeric,floor((clock.now::numeric-s.next_start_ms-s.window_ms-s.lateness_ms)/s.window_ms)+1)::int AS n
              FROM analytics.calculator_schedules s JOIN metadata.tenants t USING(tenant_id)
              JOIN analytics.calculator_configuration_revisions r ON r.tenant_id=s.tenant_id AND r.schedule_id=s.schedule_id
              JOIN analytics.calculator_configurations c ON c.tenant_id=r.tenant_id AND c.configuration_id=r.configuration_id AND c.active_revision=r.revision CROSS JOIN clock
              WHERE t.is_active AND s.next_start_ms::numeric+s.window_ms+s.lateness_ms<=clock.now
              ORDER BY s.next_start_ms,s.tenant_id,s.schedule_id FOR UPDATE OF s,c SKIP LOCKED LIMIT 1
            ), saved AS (
              INSERT INTO analytics.calculator_work(tenant_id,evaluation_id,schedule_id,start_ms,request)
              SELECT tenant_id,'scheduled:'||encode(sha256(convert_to(jsonb_build_array(tenant_id,schedule_id,next_start_ms+nth*window_ms)::text,'UTF8')),'hex'),schedule_id,next_start_ms+nth*window_ms,
                jsonb_build_object('scope',scope_asset_id,'calculator',calculator_id,'version',calculator_version,'configuration',configuration_id,
                  'start',next_start_ms+nth*window_ms,'end',next_start_ms+(nth+1)*window_ms,'finalize',next_start_ms+(nth+1)*window_ms+lateness_ms,
                  'known',next_start_ms+(nth+1)*window_ms+lateness_ms,'evaluated',now,'minimum',minimum_samples,'target',target_asset_id)
              FROM candidate CROSS JOIN LATERAL generate_series(0,n-1) nth RETURNING 1
            ), advanced AS (
              UPDATE analytics.calculator_schedules s SET next_start_ms=c.next_start_ms+c.n*c.window_ms FROM candidate c
              WHERE s.tenant_id=c.tenant_id AND s.schedule_id=c.schedule_id AND (SELECT count(*) FROM saved)=c.n RETURNING 1
            ) SELECT count(*) AS "count!" FROM saved
        "#).fetch_one(&self.0).await.map_err(|_|CalculatorWorkError)
    }
    /// Claims one eligible window; expired third attempts become retained exhaustion.
    /// Returns none when eligible configuration heads are locked as well as when
    /// no window is due. Callers retry on a later turn without assuming an empty queue.
    /// # Errors
    /// Returns bounded storage errors; original request is never replaced.
    pub async fn claim(&self) -> Result<Option<CalculatorWorkLease>, CalculatorWorkError> {
        sqlx::query!(r#"
            UPDATE analytics.calculator_work w SET status='exhausted'
            FROM (SELECT tenant_id,evaluation_id FROM analytics.calculator_work WHERE status='pending' AND generation=3
              AND lease_until_ms<=floor(extract(epoch FROM clock_timestamp())*1000)::bigint
              ORDER BY tenant_id,evaluation_id FOR UPDATE SKIP LOCKED LIMIT 32) expired
            WHERE w.tenant_id=expired.tenant_id AND w.evaluation_id=expired.evaluation_id
        "#).execute(&self.0).await.map_err(|_|CalculatorWorkError)?;
        let row=sqlx::query!(r#"
            WITH candidate AS (
              SELECT w.tenant_id,w.evaluation_id FROM analytics.calculator_work w JOIN metadata.tenants t USING(tenant_id)
              JOIN analytics.calculator_configuration_revisions r ON r.tenant_id=w.tenant_id AND r.schedule_id=w.schedule_id
              JOIN analytics.calculator_configurations config ON config.tenant_id=r.tenant_id AND config.configuration_id=r.configuration_id AND config.active_revision=r.revision
              WHERE t.is_active AND w.status='pending' AND w.generation<3 AND (w.lease_until_ms IS NULL OR w.lease_until_ms<=floor(extract(epoch FROM clock_timestamp())*1000)::bigint)
              ORDER BY w.start_ms,w.tenant_id,w.evaluation_id FOR UPDATE OF w,config SKIP LOCKED LIMIT 1
            ) UPDATE analytics.calculator_work w SET generation=generation+1,lease_until_ms=floor(extract(epoch FROM clock_timestamp())*1000)::bigint+30000
            FROM candidate c WHERE w.tenant_id=c.tenant_id AND w.evaluation_id=c.evaluation_id
            RETURNING w.tenant_id,w.evaluation_id,w.request,w.generation
        "#).fetch_optional(&self.0).await.map_err(|_|CalculatorWorkError)?;
        row.map(|row| {
            Ok(CalculatorWorkLease {
                request: decode(&row.tenant_id, &row.evaluation_id, &row.request)?,
                generation: row.generation,
            })
        })
        .transpose()
    }
    /// Appends terminal output and marks done under the current unexpired fence.
    /// # Errors
    /// Expired/stale/inactive claims or mismatched records cannot commit outputs.
    pub async fn complete(
        &self,
        lease: &CalculatorWorkLease,
        record: &CalculatorEvaluationRecord,
    ) -> Result<(), CalculatorWorkError> {
        let request = &lease.request;
        let tenant = request.evaluation.tenant_id().as_str();
        let id = request.evaluation.id().as_str();
        let generation = lease.generation;
        if record.evaluation() != &request.evaluation
            || record.key().configuration() != &request.configuration
            || record.key().manifest().window() != request.window
            || record.key().manifest().known_at() != request.known_at
            || record.minimum_samples() != request.minimum_samples
            || record
                .outputs()
                .iter()
                .any(|output| output.property().asset_id() != &request.target)
        {
            return Err(CalculatorWorkError);
        }
        let mut tx = self.0.begin().await.map_err(|_| CalculatorWorkError)?;
        let saved=sqlx::query_scalar!(r#"
            SELECT w.request FROM analytics.calculator_work w JOIN metadata.tenants t USING(tenant_id)
            WHERE w.tenant_id=$1 AND w.evaluation_id=$2 AND w.generation=$3 AND w.status='pending' AND t.is_active
              AND w.lease_until_ms>floor(extract(epoch FROM clock_timestamp())*1000)::bigint FOR UPDATE OF w,t
        "#,tenant,id,generation).fetch_optional(&mut *tx).await.map_err(|_|CalculatorWorkError)?;
        if saved != Some(request_json(request)) {
            return Err(CalculatorWorkError);
        }
        append_record(&mut tx, record)
            .await
            .map_err(|_| CalculatorWorkError)?;
        let updated=sqlx::query!("UPDATE analytics.calculator_work SET status='done' WHERE tenant_id=$1 AND evaluation_id=$2 AND generation=$3 AND status='pending' AND lease_until_ms>floor(extract(epoch FROM clock_timestamp())*1000)::bigint",tenant,id,generation).execute(&mut *tx).await.map_err(|_|CalculatorWorkError)?;
        if updated.rows_affected() != 1 {
            return Err(CalculatorWorkError);
        }
        tx.commit().await.map_err(|_| CalculatorWorkError)
    }
}
fn decode(
    tenant: &str,
    id: &str,
    value: &Value,
) -> Result<CalculatorRunRequest, CalculatorWorkError> {
    let convert = || -> Result<CalculatorRunRequest, Box<dyn std::error::Error>> {
        let time = |key| {
            value[key]
                .as_i64()
                .map(UnixTimeMillis::new)
                .ok_or(CalculatorWorkError)
        };
        let text = |key| value[key].as_str().ok_or(CalculatorWorkError);
        Ok(CalculatorRunRequest {
            evaluation: CalculatorEvaluation::new(
                CalculatorEvaluationId::new(id)?,
                TenantId::new(tenant)?,
                AssetId::new(text("scope")?)?,
                CalculatorRef::new(
                    CalculatorId::new(text("calculator")?)?,
                    CalculatorVersion::new(text("version")?)?,
                ),
                time("end")?,
                time("evaluated")?,
            ),
            configuration: CalculatorConfigurationId::new(text("configuration")?)?,
            window: CalculatorWindow::new(time("start")?, time("end")?, time("finalize")?)?,
            known_at: time("known")?,
            minimum_samples: u16::try_from(value["minimum"].as_u64().ok_or(CalculatorWorkError)?)?,
            target: AssetId::new(text("target")?)?,
        })
    };
    convert().map_err(|_| CalculatorWorkError)
}
/// Bounded schedule, storage or fence failure; no raw process diagnostics.
#[derive(Clone, Copy, Debug)]
pub struct CalculatorWorkError;
impl std::fmt::Display for CalculatorWorkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("calculator scheduling or claim rejected")
    }
}
impl std::error::Error for CalculatorWorkError {}
