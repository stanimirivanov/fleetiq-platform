//! Explicit calculator scheduling and alert lifecycle consumption; no automatic DDL or delivery.
use fleetiq_analytics::{CalculatorPreparation, calculate_prepared};
use fleetiq_postgres::{
    PostgresAlertLifecycle, PostgresCalculatorPreparation, PostgresCalculatorWork,
};
use std::{error::Error, time::Duration};

/// Runs bounded durable work against a checked schema until process shutdown.
/// `DATABASE_URL` is required. Only explicitly activated configuration revisions
/// run; the transitional schedule-file option is rejected.
/// # Errors
/// Returns bounded startup errors; failed turns retain their lease for retry.
pub async fn run_analytics_worker() -> Result<(), Box<dyn Error>> {
    if std::env::var_os("FLEETIQ_CALCULATOR_SCHEDULE_FILE").is_some() {
        return Err("schedule files retired; use calculator configuration API".into());
    }
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
        let alerts = PostgresAlertLifecycle::new(pool.clone());
        let shutdown = crate::runtime::shutdown_signal();
        tokio::pin!(shutdown);
        loop {
            let calculator_turn = async {
                work.seed().await?;
                if let Some(lease) = work.claim().await? {
                    let prepared = preparation.prepare(&lease.request).await.map_err(|_| "calculator preparation failed")?;
                    let record = calculate_prepared(prepared).map_err(|_| "calculator calculation failed")?;
                    work.complete(&lease, &record).await?;
                }
                Ok::<(), Box<dyn Error>>(())
            };
            // Independent bounded turns: a failed calculator must not starve
            // already retained alert evidence; shutdown cancels both transactions.
            let turn = async {
                tokio::join!(
                    tokio::time::timeout(Duration::from_secs(15), calculator_turn),
                    tokio::time::timeout(Duration::from_secs(15), alerts.advance_one()),
                )
            };
            tokio::select! {
                () = &mut shutdown => break,
                (calculator, alert) = turn => {
                    if !matches!(calculator, Ok(Ok(()))) { eprintln!("calculator turn failed; durable retry retained"); }
                    if !matches!(alert, Ok(Ok(_))) { eprintln!("alert lifecycle turn failed; durable retry retained"); }
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
