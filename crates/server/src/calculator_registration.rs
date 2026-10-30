//! Trusted operator registration; checked schema, no inline content or HTTP.
use fleetiq_analytics::{
    CalculatorDefinition, CalculatorId, CalculatorImplementation, CalculatorRef, CalculatorVersion,
};
use fleetiq_domain::{DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, TenantId};
use fleetiq_postgres::PostgresCalculatorRegistry;
use serde::Deserialize;
use std::{error::Error, io::Read, time::Duration};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Declaration {
    tenant: String,
    calculator: String,
    version: String,
    implementation: String,
    input: String,
    input_version: u32,
    output: String,
    output_version: u32,
}
fn parse(bytes: &[u8]) -> Result<CalculatorDefinition, Box<dyn Error>> {
    let d: Declaration =
        serde_json::from_slice(bytes).map_err(|_| "invalid calculator declaration")?;
    let convert = || -> Result<CalculatorDefinition, Box<dyn Error>> {
        Ok(CalculatorDefinition {
            tenant: TenantId::new(d.tenant)?,
            release: CalculatorRef::new(
                CalculatorId::new(d.calculator)?,
                CalculatorVersion::new(d.version)?,
            ),
            implementation: CalculatorImplementation::parse(&d.implementation)?,
            input: PropertyDefinitionRef::new(
                PropertyDefinitionId::new(d.input)?,
                DefinitionVersion::new(d.input_version)?,
            ),
            output: PropertyDefinitionRef::new(
                PropertyDefinitionId::new(d.output)?,
                DefinitionVersion::new(d.output_version)?,
            ),
        })
    };
    convert().map_err(|_| "invalid calculator declaration".into())
}
/// Registers one first-party declaration using `DATABASE_URL` and a bounded JSON
/// file named by `FLEETIQ_CALCULATOR_DEFINITION_FILE`. Never migrates or activates.
/// # Errors
/// Returns bounded configuration/schema/storage errors without input content.
pub async fn register_calculator_definition() -> Result<(), Box<dyn Error>> {
    let path = std::env::var_os("FLEETIQ_CALCULATOR_DEFINITION_FILE")
        .ok_or("calculator declaration file required")?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "calculator declaration unavailable")?
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "calculator declaration unreadable")?;
    if bytes.len() > 4096 {
        return Err("calculator declaration exceeds 4096 bytes".into());
    }
    let definition = parse(&bytes)?;
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
        PostgresCalculatorRegistry::new(pool.clone())
            .register(&definition)
            .await
            .map_err(|_| "calculator registration rejected")?;
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
    fn declaration_is_exact_and_rejects_unknown_code_or_fields() {
        let valid = r#"{"tenant":"t","calculator":"mean","version":"1","implementation":"window-mean-v1","input":"temperature","input_version":1,"output":"mean","output_version":1}"#;
        assert!(parse(valid.as_bytes()).is_ok());
        for invalid in [
            valid.replace("window-mean-v1", "plugin"),
            valid.replace("\"input_version\":1", "\"input_version\":0"),
            valid.replace("\"tenant\":\"t\"", "\"tenant\":\"\""),
            valid.replace("\"tenant\":\"t\"", "\"extra\":true,\"tenant\":\"t\""),
        ] {
            assert!(parse(invalid.as_bytes()).is_err());
        }
    }
}
