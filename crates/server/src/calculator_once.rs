//! Trusted one-shot execution using a bounded operator request file.
use fleetiq_analytics::{
    CalculatorConfigurationId, CalculatorEvaluation, CalculatorEvaluationId, CalculatorId,
    CalculatorRef, CalculatorRunRequest, CalculatorVersion, CalculatorWindow, run_calculator_once,
};
use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use fleetiq_postgres::{PostgresCalculatorEvaluationStore, PostgresCalculatorPreparation};
use serde::Deserialize;
use std::{error::Error, io::Read, time::Duration};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    tenant: String,
    scope: String,
    calculator: String,
    version: String,
    evaluation: String,
    configuration: String,
    target: String,
    start: i64,
    end: i64,
    finalize: i64,
    known: i64,
    evaluated: i64,
    minimum: u16,
}
fn parse(bytes: &[u8]) -> Result<CalculatorRunRequest, Box<dyn Error>> {
    let input: Request = serde_json::from_slice(bytes).map_err(|_| "invalid calculator request")?;
    let convert = || -> Result<CalculatorRunRequest, Box<dyn Error>> {
        let time = UnixTimeMillis::new;
        let window =
            CalculatorWindow::new(time(input.start), time(input.end), time(input.finalize))?;
        if input.known < input.finalize
            || input.evaluated < input.known
            || !(1..=1024).contains(&input.minimum)
        {
            return Err("invalid bounds".into());
        }
        Ok(CalculatorRunRequest {
            evaluation: CalculatorEvaluation::new(
                CalculatorEvaluationId::new(input.evaluation)?,
                TenantId::new(input.tenant)?,
                AssetId::new(input.scope)?,
                CalculatorRef::new(
                    CalculatorId::new(input.calculator)?,
                    CalculatorVersion::new(input.version)?,
                ),
                time(input.end),
                time(input.evaluated),
            ),
            configuration: CalculatorConfigurationId::new(input.configuration)?,
            window,
            known_at: time(input.known),
            minimum_samples: input.minimum,
            target: AssetId::new(input.target)?,
        })
    };
    convert().map_err(|_| "invalid calculator request".into())
}
/// Runs one reviewed calculator and exits without migration, HTTP or scheduling.
/// Reads `DATABASE_URL` and `FLEETIQ_CALCULATOR_REQUEST_FILE`; retain the exact
/// request after timeout or ambiguous storage results to reuse committed inputs.
/// # Errors
/// Returns bounded errors without file content, paths or connection strings.
pub async fn calculate_once() -> Result<(), Box<dyn Error>> {
    let path = std::env::var_os("FLEETIQ_CALCULATOR_REQUEST_FILE")
        .ok_or("calculator request file required")?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "calculator request unavailable")?
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "calculator request unreadable")?;
    if bytes.len() > 4096 {
        return Err("calculator request too large".into());
    }
    let request = parse(&bytes)?;
    let url = std::env::var("DATABASE_URL").map_err(|_| "database configuration required")?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
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
        crate::SchemaMode::Check
            .prepare(&pool)
            .await
            .map_err(|_| "calculator schema incompatible")?;
        tokio::time::timeout(
            Duration::from_secs(15),
            run_calculator_once(
                &PostgresCalculatorPreparation::new(pool.clone()),
                &PostgresCalculatorEvaluationStore::new(pool.clone()),
                &request,
            ),
        )
        .await
        .map_err(|_| "calculator turn deadline exceeded")??;
        Ok::<(), Box<dyn Error>>(())
    }
    .await;
    pool.close().await;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_request_rejects_unfinalized_windows_and_unknown_fields() {
        let valid = r#"{"tenant":"t","scope":"s","calculator":"mean","version":"1","evaluation":"e","configuration":"c","target":"s","start":0,"end":100,"finalize":100,"known":100,"evaluated":101,"minimum":1}"#;
        assert!(parse(valid.as_bytes()).is_ok());
        for invalid in [
            valid.replace("\"known\":100", "\"known\":99"),
            valid.replace("\"minimum\":1", "\"minimum\":0"),
            valid.replace("\"tenant\":\"t\"", "\"tenant\":\"t\",\"extra\":true"),
        ] {
            assert!(parse(invalid.as_bytes()).is_err());
        }
    }
}
