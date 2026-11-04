//! Verify the checked-schema process exits while a live response remains open.
#![cfg(unix)]

use openidconnect::reqwest;
use sqlx::{ConnectOptions, PgPool};
use std::{error::Error, process::Stdio, time::Duration};
use tokio::{net::TcpListener, process::Command, time::timeout};

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn sigterm_exits_with_an_open_live_response(pool: PgPool) -> Result<(), Box<dyn Error>> {
    sqlx::raw_sql(include_str!(
        "../../postgres/tests/property_history/topology.sql"
    ))
    .execute(&pool)
    .await?;

    let reservation = TcpListener::bind("127.0.0.1:0").await?;
    let address = reservation.local_addr()?;
    drop(reservation);
    let token = "a".repeat(64);
    let mut child = Command::new(env!("CARGO_BIN_EXE_fleetiq-server"))
        .args(["serve", "--check-schema"])
        .env(
            "DATABASE_URL",
            pool.connect_options().to_url_lossy().as_str(),
        )
        .env("FLEETIQ_BIND_ADDRESS", address.to_string())
        .env("FLEETIQ_HTTP_BEARER_TOKEN", &token)
        .env("FLEETIQ_HTTP_IDENTITY_AUTHORITY", "workload")
        .env("FLEETIQ_HTTP_IDENTITY_SUBJECT", "operator")
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let client = reqwest::Client::new();
    let url = format!(
        "http://{address}/api/v1/tenants/tenant-a/assets/asset-a/properties/electrical.voltage/versions/1/live"
    );
    let mut response = timeout(Duration::from_secs(10), async {
        loop {
            assert!(
                child.try_wait()?.is_none(),
                "server exited before readiness"
            );
            match client.get(&url).bearer_auth(&token).send().await {
                Ok(response) => return Ok::<_, Box<dyn Error>>(response),
                Err(error) if error.is_connect() => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
    })
    .await??;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert!(
        timeout(Duration::from_secs(5), response.chunk())
            .await??
            .is_some()
    );

    let pid = child.id().ok_or("server already exited")?.to_string();
    assert!(
        Command::new("kill")
            .args(["-TERM", &pid])
            .status()
            .await?
            .success()
    );
    let output = timeout(Duration::from_secs(9), child.wait_with_output())
        .await
        .expect("server must exit before the local supervisor's 10-second deadline")?;
    drop(response);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        std::str::from_utf8(&output.stderr)?.trim(),
        "HTTP shutdown deadline exceeded"
    );
    Ok(())
}
