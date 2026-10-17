//! Compatibility is a read-only deployment boundary, not an implicit migration.
use fleetiq_postgres::{MIGRATOR, SchemaCompatibilityError, apply_migrations, check_schema};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::error::Error;

#[sqlx::test(migrations = false)]
async fn absent_history_is_rejected_without_creating_it(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    assert!(matches!(
        check_schema(&pool).await,
        Err(SchemaCompatibilityError::MissingLedger)
    ));
    let ledger: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public._sqlx_migrations')::text")
            .fetch_one(&pool)
            .await?;
    assert!(ledger.is_none());
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn rejects_missing_unknown_modified_and_failed_history(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    check_schema(&pool).await?;
    let last = MIGRATOR.iter().last().expect("nonempty inventory");
    sqlx::query("DELETE FROM public._sqlx_migrations WHERE version = $1")
        .bind(last.version)
        .execute(&pool)
        .await?;
    assert!(
        matches!(check_schema(&pool).await, Err(SchemaCompatibilityError::MissingMigration { version }) if version == last.version)
    );
    sqlx::query("INSERT INTO public._sqlx_migrations (version, description, checksum, success, execution_time) VALUES ($1, 'fixture', $2, TRUE, 0)")
        .bind(last.version).bind(last.checksum.as_ref()).execute(&pool).await?;
    check_schema(&pool).await?;
    sqlx::query(
        "UPDATE public._sqlx_migrations SET checksum = decode('00', 'hex') WHERE version = $1",
    )
    .bind(last.version)
    .execute(&pool)
    .await?;
    assert!(matches!(
        check_schema(&pool).await,
        Err(SchemaCompatibilityError::ChecksumMismatch { .. })
    ));
    sqlx::query("UPDATE public._sqlx_migrations SET checksum = $1 || decode(repeat('00', 1024), 'hex') WHERE version = $2")
        .bind(last.checksum.as_ref()).bind(last.version).execute(&pool).await?;
    assert!(matches!(
        check_schema(&pool).await,
        Err(SchemaCompatibilityError::ChecksumMismatch { .. })
    ));
    sqlx::query(
        "UPDATE public._sqlx_migrations SET checksum = $1, success = FALSE WHERE version = $2",
    )
    .bind(last.checksum.as_ref())
    .bind(last.version)
    .execute(&pool)
    .await?;
    assert!(matches!(
        check_schema(&pool).await,
        Err(SchemaCompatibilityError::FailedMigration { .. })
    ));
    sqlx::query("UPDATE public._sqlx_migrations SET success = TRUE WHERE version = $1")
        .bind(last.version)
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO public._sqlx_migrations (version, description, checksum, success, execution_time) VALUES ($1, 'future', decode('00', 'hex'), TRUE, 0)")
        .bind(last.version + 1).execute(&pool).await?;
    assert!(matches!(
        check_schema(&pool).await,
        Err(SchemaCompatibilityError::UnknownMigration { .. })
    ));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn builtin_read_only_role_checks_exact_history_without_ddl(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    // PostgreSQL's built-in data reader is not the schema owner and has no CREATE
    // authority. Read-only transactions additionally reject even attempted DDL.
    let options = pool.connect_options().as_ref().clone().options([
        ("role", "pg_read_all_data"),
        ("default_transaction_read_only", "on"),
    ]);
    let restricted = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&restricted)
        .await?;
    assert_eq!(role, "pg_read_all_data");
    check_schema(&restricted).await?;
    assert!(
        sqlx::query("CREATE TABLE public.forbidden_ddl (id integer)")
            .execute(&restricted)
            .await
            .is_err()
    );
    restricted.close().await;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn inaccessible_history_is_not_compatible_and_diagnostics_are_redacted(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let options = pool
        .connect_options()
        .as_ref()
        .clone()
        .options([("role", "pg_signal_backend")]);
    let restricted = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let error = check_schema(&restricted)
        .await
        .expect_err("no ledger privilege");
    assert!(matches!(error, SchemaCompatibilityError::Database(_)));
    assert_eq!(format!("{error:?}"), "migration history could not be read");
    restricted.close().await;
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn concurrent_locked_migrations_apply_once_and_repeat_safely(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let (first, second) = tokio::join!(apply_migrations(&pool), apply_migrations(&pool));
    first?;
    second?;
    apply_migrations(&pool).await?;
    check_schema(&pool).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM public._sqlx_migrations")
        .fetch_one(&pool)
        .await?;
    assert_eq!(usize::try_from(count)?, MIGRATOR.iter().count());
    Ok(())
}

#[sqlx::test(migrations = false)]
async fn failed_transaction_does_not_claim_applied_history(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    // An owned empty schema forces the first real migration to fail. Its SQL
    // and ledger entry remain transactional; no historical file is modified.
    sqlx::query("CREATE SCHEMA metadata").execute(&pool).await?;
    assert!(apply_migrations(&pool).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM public._sqlx_migrations")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 0);
    assert!(matches!(
        check_schema(&pool).await,
        Err(SchemaCompatibilityError::MissingMigration { .. })
    ));
    Ok(())
}
