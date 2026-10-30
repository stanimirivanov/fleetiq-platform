//! Explicit trusted fixed-window scheduling; no HTTP activation or automatic DDL.
use fleetiq_analytics::{
    CalculatorConfigurationId, CalculatorId, CalculatorPreparation, CalculatorRef,
    CalculatorSchedule, CalculatorVersion, calculate_mean,
};
use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use fleetiq_postgres::{PostgresCalculatorPreparation, PostgresCalculatorWork};
use serde::Deserialize;
use std::{error::Error, io::Read, time::Duration};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerInput {
    tenant: String,
    schedule: String,
    calculator: String,
    version: String,
    configuration: String,
    scope: String,
    target: String,
    first_start: i64,
    window_ms: u32,
    lateness_ms: u32,
    minimum: u16,
}
fn parse(bytes: &[u8]) -> Result<CalculatorSchedule, Box<dyn Error>> {
    let convert = || -> Result<CalculatorSchedule, Box<dyn Error>> {
        let s: WorkerInput = serde_json::from_slice(bytes)?;
        let schedule = CalculatorSchedule {
            tenant: TenantId::new(s.tenant)?,
            id: CalculatorConfigurationId::new(s.schedule)?,
            calculator: CalculatorRef::new(
                CalculatorId::new(s.calculator)?,
                CalculatorVersion::new(s.version)?,
            ),
            configuration: CalculatorConfigurationId::new(s.configuration)?,
            scope: AssetId::new(s.scope)?,
            target: AssetId::new(s.target)?,
            first_start: UnixTimeMillis::new(s.first_start),
            window_ms: s.window_ms,
            lateness_ms: s.lateness_ms,
            minimum_samples: s.minimum,
        };
        if !schedule.is_valid() {
            return Err("invalid bounds".into());
        }
        Ok(schedule)
    };
    convert().map_err(|_| "invalid calculator schedule".into())
}
/// Runs bounded durable work against a checked schema until process shutdown.
/// `DATABASE_URL` is required; optional `FLEETIQ_CALCULATOR_SCHEDULE_FILE` installs
/// one immutable trusted schedule before processing all active-tenant schedules.
/// # Errors
/// Returns bounded startup errors; failed turns retain their lease for retry.
pub async fn run_analytics_worker() -> Result<(), Box<dyn Error>> {
    let schedule = if let Some(path) = std::env::var_os("FLEETIQ_CALCULATOR_SCHEDULE_FILE") {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|_| "calculator schedule unavailable")?
            .take(4097)
            .read_to_end(&mut bytes)
            .map_err(|_| "calculator schedule unreadable")?;
        if bytes.len() > 4096 {
            return Err("calculator schedule too large".into());
        }
        Some(parse(&bytes)?)
    } else {
        None
    };
    let url = std::env::var("DATABASE_URL").map_err(|_| "database configuration required")?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(3))
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET statement_timeout = '5s'")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .map_err(|_| "calculator database unavailable")?;
    let result = async {
        crate::SchemaMode::Check.prepare(&pool).await.map_err(|_| "calculator schema incompatible")?;
        let work = PostgresCalculatorWork::new(pool.clone());
        let preparation = PostgresCalculatorPreparation::new(pool.clone());
        if let Some(schedule) = schedule { work.register(&schedule).await?; }
        let shutdown = crate::runtime::shutdown_signal();
        tokio::pin!(shutdown);
        loop {
            let turn = async {
                work.seed().await?;
                if let Some(lease) = work.claim().await? {
                    let prepared = preparation.prepare(&lease.request).await.map_err(|_| "calculator preparation failed")?;
                    let record = calculate_mean(prepared).map_err(|_| "calculator calculation failed")?;
                    work.complete(&lease, &record).await?;
                }
                Ok::<(), Box<dyn Error>>(())
            };
            tokio::select! {
                () = &mut shutdown => break,
                result = tokio::time::timeout(Duration::from_secs(15), turn) => {
                    if !matches!(result, Ok(Ok(()))) { eprintln!("calculator turn failed; durable retry retained"); }
                }
            }
            tokio::select! {
                () = &mut shutdown => break,
                () = tokio::time::sleep(Duration::from_millis(250)) => {}
            }
        }
        Ok::<(), Box<dyn Error>>(())
    }.await;
    pool.close().await;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schedule_bounds_and_unknown_fields_are_rejected() {
        let valid = r#"{"tenant":"t","schedule":"s","calculator":"mean","version":"1","configuration":"c","scope":"a","target":"b","first_start":0,"window_ms":1000,"lateness_ms":0,"minimum":1}"#;
        assert!(parse(valid.as_bytes()).is_ok());
        for invalid in [
            valid.replace("\"window_ms\":1000", "\"window_ms\":0"),
            valid.replace("\"window_ms\":1000", "\"window_ms\":604800001"),
            valid.replace("\"minimum\":1", "\"minimum\":0"),
            valid.replace("\"first_start\":0", "\"first_start\":9223372036854775807"),
            valid.replace("\"tenant\":\"t\"", "\"tenant\":\"t\",\"extra\":true"),
        ] {
            assert!(parse(invalid.as_bytes()).is_err());
        }
    }
}
