//! Exercise the real checked-schema command, durable effects and graceful stop.
use sqlx::{ConnectOptions, PgPool};
use std::{error::Error, process::Stdio, time::Duration};
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn analytics_worker_consumes_selected_rules_without_starting_delivery(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    sqlx::raw_sql(include_str!(
        "../../postgres/tests/property_history/topology.sql"
    ))
    .execute(&pool)
    .await?;
    sqlx::raw_sql(include_str!("alert_worker/seed.sql"))
        .execute(&pool)
        .await?;
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_fleetiq-server"))
        .arg("analytics-worker")
        .env(
            "DATABASE_URL",
            pool.connect_options().to_url_lossy().as_str(),
        )
        .env_remove("FLEETIQ_CALCULATOR_SCHEDULE_FILE")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            assert!(
                child.try_wait()?.is_none(),
                "worker exited before consuming evidence"
            );
            let ready: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM analytics.alert_notification_intents)",
            )
            .fetch_one(&pool)
            .await?;
            if ready {
                break;
            }
            tokio::task::yield_now().await;
        }
        Ok::<(), Box<dyn Error>>(())
    })
    .await??;
    let pid = child.id().ok_or("worker already exited")?.to_string();
    assert!(
        tokio::process::Command::new("kill")
            .args(["-TERM", &pid])
            .status()
            .await?
            .success()
    );
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait()).await??;
    assert!(status.success());
    let state:(String,i64,i64)=sqlx::query_as("SELECT phase,(SELECT count(*) FROM analytics.alert_transitions),(SELECT count(*) FROM analytics.alert_notification_intents) FROM analytics.alert_lifecycles").fetch_one(&pool).await?;
    assert_eq!(state, ("firing".into(), 1, 1));
    Ok(())
}
