//! Exercises the published process commands, not an alternate test migration path.
use fleetiq_server::{MigrationConfig, migrate_database, migration_inventory};
use sqlx::{ConnectOptions, PgPool};
use std::{error::Error, process::Command};

#[test]
fn inventory_is_deterministic_ordered_and_needs_no_configuration() -> Result<(), Box<dyn Error>> {
    let expected = migration_inventory()?;
    assert_eq!(expected, migration_inventory()?);
    let inventory: serde_json::Value = serde_json::from_str(&expected)?;
    assert_eq!(inventory["format"], "fleetiq-migrations.v1");
    assert_eq!(inventory["checksum_algorithm"], "sha384");
    let entries = inventory["migrations"].as_array().expect("migration array");
    assert_eq!(entries.len(), fleetiq_postgres::MIGRATOR.iter().count());
    let mut previous = 0;
    for entry in entries {
        let version = entry["version"]
            .as_str()
            .expect("decimal version")
            .parse::<i64>()?;
        assert!(version > previous);
        previous = version;
        assert_eq!(entry["checksum"].as_str().expect("checksum").len(), 96);
    }
    let output = Command::new(env!("CARGO_BIN_EXE_fleetiq-server"))
        .arg("migration-inventory")
        .env_remove("DATABASE_URL")
        .output()?;
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout)?.trim(), expected);
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn explicit_process_migrates_without_http_or_identity_and_repeats_safely(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let database = pool.connect_options().to_url_lossy().to_string();
    for _ in 0..2 {
        let output = Command::new(env!("CARGO_BIN_EXE_fleetiq-server"))
            .arg("migrate")
            .env("DATABASE_URL", &database)
            .env("FLEETIQ_BIND_ADDRESS", "invalid")
            .env(
                "FLEETIQ_OIDC_CLIENT_SECRET",
                "private-not-required-for-migration",
            )
            .output()?;
        assert!(
            output.status.success(),
            "migration failed without exposing child diagnostics"
        );
        assert_eq!(
            String::from_utf8(output.stdout)?.trim(),
            "embedded migrations applied and verified"
        );
        // Compare only byte counts so failures never print private child diagnostics.
        assert_eq!(
            output.stderr.len(),
            0,
            "migration must not emit diagnostics"
        );
    }
    fleetiq_postgres::check_schema(&pool).await?;
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn migration_failure_exits_and_releases_connections_for_reviewed_recovery(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    sqlx::query("CREATE SCHEMA metadata").execute(&pool).await?;
    let config = MigrationConfig::from_values(pool.connect_options().to_url_lossy().to_string())?;
    let error = migrate_database(config.clone())
        .await
        .expect_err("existing schema blocks migration");
    assert!(!format!("{error:?} {error}").contains("CREATE SCHEMA"));
    // Only our empty fixture schema is removed. A fresh migration process must
    // acquire the SQLx lock after the failed command closed its sole connection.
    sqlx::query("DROP SCHEMA metadata").execute(&pool).await?;
    migrate_database(config).await?;
    fleetiq_postgres::check_schema(&pool).await?;
    Ok(())
}

#[test]
fn additional_ca_configuration_without_browser_mode_is_rejected_before_database_access()
-> Result<(), Box<dyn Error>> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fleetiq-server"));
    command
        .args(["serve", "--check-schema"])
        .env(
            "DATABASE_URL",
            "postgres://unused:private-db-value@127.0.0.1:1/unused",
        )
        .env("FLEETIQ_BIND_ADDRESS", "127.0.0.1:0")
        .env("FLEETIQ_OIDC_ADDITIONAL_CA_BUNDLE", "private-mount-path");
    for name in [
        "FLEETIQ_BROWSER_ORIGIN",
        "FLEETIQ_OIDC_ISSUER",
        "FLEETIQ_OIDC_CLIENT_ID",
        "FLEETIQ_OIDC_CLIENT_SECRET",
        "FLEETIQ_OIDC_IDENTITY_AUTHORITY",
        "FLEETIQ_HTTP_BEARER_TOKEN",
        "FLEETIQ_HTTP_IDENTITY_AUTHORITY",
        "FLEETIQ_HTTP_IDENTITY_SUBJECT",
    ] {
        command.env_remove(name);
    }
    let output = command.output()?;
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr)?;
    assert!(error.contains("must be configured together"));
    assert!(!error.contains("private-mount-path") && !error.contains("private-db-value"));
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn migration_session_ignores_a_shadow_ledger_and_pins_public_history(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    sqlx::raw_sql("CREATE SCHEMA shadow_ledger; CREATE TABLE shadow_ledger._sqlx_migrations (version bigint PRIMARY KEY, checksum bytea NOT NULL, success boolean NOT NULL); INSERT INTO shadow_ledger._sqlx_migrations VALUES (999999, decode('00','hex'), FALSE);").execute(&pool).await?;
    // Preserve startup options explicitly; to_url_lossy intentionally omits them.
    let mut url = pool.connect_options().to_url_lossy();
    url.query_pairs_mut()
        .append_pair("options", "-c search_path=shadow_ledger,public");
    let url = url.to_string();
    let control = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await?;
    let selected: i64 = sqlx::query_scalar("SELECT version FROM _sqlx_migrations")
        .fetch_one(&control)
        .await?;
    assert_eq!(
        selected, 999_999,
        "fixture must actually select the shadow ledger"
    );
    control.close().await;
    migrate_database(MigrationConfig::from_values(url)?).await?;
    fleetiq_postgres::check_schema(&pool).await?;
    let shadow: i64 = sqlx::query_scalar("SELECT version FROM shadow_ledger._sqlx_migrations")
        .fetch_one(&pool)
        .await?;
    assert_eq!(shadow, 999_999);
    Ok(())
}
