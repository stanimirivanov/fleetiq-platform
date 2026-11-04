//! Real notification process uses checked schema, retained state and owned loopback I/O.
use axum::{Router, body::Bytes, http::StatusCode, routing::post};
use sqlx::{ConnectOptions, PgPool};
use std::{
    error::Error,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
async fn stop(child: &mut tokio::process::Child) -> Result<(), Box<dyn Error>> {
    let pid = child.id().ok_or("worker exited")?.to_string();
    assert!(
        tokio::process::Command::new("kill")
            .args(["-TERM", &pid])
            .status()
            .await?
            .success()
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await??
            .success()
    );
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn explicit_process_delivers_and_refreshes_after_restart_without_ddl(
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
    assert!(
        fleetiq_postgres::PostgresAlertLifecycle::new(pool.clone())
            .advance_one()
            .await?
    );
    let payloads = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let received = Arc::new(tokio::sync::Notify::new());
    let values = payloads.clone();
    let notice = received.clone();
    let app = Router::new().route(
        "/api/v2/alerts",
        post(move |body: Bytes| {
            values
                .lock()
                .unwrap()
                .push(serde_json::from_slice(&body).unwrap());
            notice.notify_one();
            async { StatusCode::OK }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}/api/v2/alerts", listener.local_addr()?);
    let receiver = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    for _ in 0..2 {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_fleetiq-server"))
            .arg("notification-worker")
            .env(
                "DATABASE_URL",
                pool.connect_options().to_url_lossy().as_str(),
            )
            .env("FLEETIQ_ALERTMANAGER_URL", &endpoint)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        tokio::time::timeout(Duration::from_secs(10), received.notified()).await?;
        tokio::time::timeout(Duration::from_secs(5),async{loop{let done:bool=sqlx::query_scalar("SELECT accepted_version=target_version AND lease_until IS NULL FROM analytics.alert_notification_delivery").fetch_one(&pool).await?;if done{break;}tokio::task::yield_now().await;}Ok::<(),sqlx::Error>(())}).await??;
        stop(&mut child).await?;
        sqlx::query("UPDATE analytics.alert_notification_delivery SET next_attempt_at=clock_timestamp()-interval '1 second'").execute(&pool).await?;
    }
    {
        let seen = payloads.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0][0]["labels"], seen[1][0]["labels"]);
        assert_eq!(seen[0][0]["startsAt"], seen[1][0]["startsAt"]);
    }
    let intents: i64 =
        sqlx::query_scalar("SELECT count(*) FROM analytics.alert_notification_intents")
            .fetch_one(&pool)
            .await?;
    assert_eq!(intents, 1);
    receiver.abort();
    Ok(())
}
#[test]
fn missing_endpoint_fails_before_database_access_with_redacted_startup_error() {
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_fleetiq-server"))
        .arg("notification-worker")
        .env_remove("FLEETIQ_ALERTMANAGER_URL")
        .env_remove("DATABASE_URL")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(
        String::from_utf8(result.stderr).unwrap().trim(),
        "Alertmanager endpoint required"
    );
}
