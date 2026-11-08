//! Process and HTTP coverage for the trusted numeric summary command.
use reqwest::{Client, StatusCode};
use serde_json::Value;
use sqlx::{ConnectOptions, PgPool};
use std::{error::Error, process::Output, time::Duration};
use tokio::{net::TcpListener, process::Command, time::timeout};

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fleetiq-server"));
    command
        .arg("summarize-once")
        .env_clear()
        .env("FLEETIQ_SUMMARY_TENANT_ID", "tenant-a")
        .env("FLEETIQ_SUMMARY_ASSET_ID", "asset-a")
        .env("FLEETIQ_SUMMARY_PROPERTY_ID", "electrical.voltage")
        .env("FLEETIQ_SUMMARY_PROPERTY_VERSION", "1")
        .env("FLEETIQ_SUMMARY_START_MS", "0")
        .env("FLEETIQ_SUMMARY_END_MS", "3000")
        .env("FLEETIQ_SUMMARY_RESOLUTION_MS", "1000")
        .kill_on_drop(true);
    command
}

fn assert_failure(output: &Output, message: &str) {
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout.len(), 0);
    assert_eq!(std::str::from_utf8(&output.stderr).unwrap().trim(), message);
}

#[tokio::test]
async fn rejects_invalid_target_before_database_access() {
    for (key, value) in [
        ("FLEETIQ_SUMMARY_TENANT_ID", ""),
        ("FLEETIQ_SUMMARY_ASSET_ID", ""),
        ("FLEETIQ_SUMMARY_PROPERTY_ID", ""),
        ("FLEETIQ_SUMMARY_PROPERTY_VERSION", "0"),
        ("FLEETIQ_SUMMARY_START_MS", "1"),
        ("FLEETIQ_SUMMARY_END_MS", "9223372036854775808"),
        ("FLEETIQ_SUMMARY_RESOLUTION_MS", "0"),
        ("FLEETIQ_SUMMARY_MANIFEST_ID", "1"),
        ("FLEETIQ_SUMMARY_RESTORE_GENERATION", "1"),
    ] {
        assert_failure(
            &command().env(key, value).output().await.unwrap(),
            "invalid numeric summary target",
        );
    }
    assert_failure(
        &command().output().await.unwrap(),
        "database configuration required",
    );
    assert_failure(
        &command().arg("inline-target").output().await.unwrap(),
        "unexpected process argument",
    );
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn executable_publishes_exact_retry_for_existing_summary_api(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    sqlx::raw_sql(include_str!(
        "../../postgres/tests/property_history/topology.sql"
    ))
    .execute(&pool)
    .await?;
    let database = pool.connect_options().to_url_lossy();
    let run = || {
        let mut command = command();
        command.env("DATABASE_URL", database.as_str());
        command
    };
    assert_failure(&run().output().await?, "numeric summary rejected: Disabled");
    sqlx::query("INSERT INTO history.lifecycle_policies(tenant_id,revision,actor_id,hot_days,archive_days,max_jobs,max_bytes) VALUES('tenant-a',1,'operator-a',7,30,1,2097152)").execute(&pool).await?;
    let budgets = fleetiq_postgres::PostgresHistoryBudgets::new(pool.clone());
    let tenant = fleetiq_domain::TenantId::new("tenant-a")?;
    let fleetiq_history::lifecycle::BudgetAdmission::Reserved(held) =
        budgets.reserve(&tenant, 2_097_152).await?
    else {
        panic!("fixture reservation");
    };
    assert_failure(
        &run().output().await?,
        "numeric summary rejected: BudgetDenied",
    );
    budgets.release(&tenant, held).await?;
    assert_failure(
        &run()
            .env("FLEETIQ_SUMMARY_MANIFEST_ID", "1")
            .env("FLEETIQ_SUMMARY_RESTORE_GENERATION", "1")
            .output()
            .await?,
        "numeric summary rejected: CoverageUnavailable",
    );
    for _ in 0..2 {
        let output = run().output().await?;
        assert!(output.status.success());
        assert_eq!(output.stderr.len(), 0);
        assert_eq!(
            std::str::from_utf8(&output.stdout)?.trim(),
            "summary series=1 revision=1"
        );
    }

    let reservation = TcpListener::bind("127.0.0.1:0").await?;
    let address = reservation.local_addr()?;
    drop(reservation);
    let token = "a".repeat(64);
    let mut server = Command::new(env!("CARGO_BIN_EXE_fleetiq-server"))
        .args(["serve", "--check-schema"])
        .env_clear()
        .env("DATABASE_URL", database.as_str())
        .env("FLEETIQ_BIND_ADDRESS", address.to_string())
        .env("FLEETIQ_HTTP_BEARER_TOKEN", &token)
        .env("FLEETIQ_HTTP_IDENTITY_AUTHORITY", "workload")
        .env("FLEETIQ_HTTP_IDENTITY_SUBJECT", "operator")
        .kill_on_drop(true)
        .spawn()?;
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()?;
    let url = format!(
        "http://{address}/api/v1/tenants/tenant-a/assets/asset-a/properties/electrical.voltage/versions/1/summaries?start_at_ms=0&end_at_ms=3000&resolution_ms=1000"
    );
    let response = timeout(Duration::from_secs(10), async {
        loop {
            assert!(
                server.try_wait()?.is_none(),
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
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&response.bytes().await?)?;
    assert_eq!(body["series_id"], "1");
    assert_eq!(body["revision"], "1");
    assert_eq!(body["buckets"][1]["sum"], "9223372036854775807");
    server.kill().await?;
    Ok(())
}
