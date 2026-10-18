use super::*;
use crate::SchemaMode;
use axum::{Router, http::StatusCode, routing::get};
use openidconnect::reqwest;
use sqlx::{ConnectOptions, PgPool};
use std::{error::Error, sync::Arc, time::Duration};
use tokio::sync::{Notify, oneshot};

#[sqlx::test(migrations = false)]
async fn missing_schema_is_rejected_before_listener_binding(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let occupied = TcpListener::bind("127.0.0.1:0").await?;
    let url = pool.connect_options().to_url_lossy().to_string();
    let config = ServerConfig::from_values(url, occupied.local_addr()?.to_string())?
        .with_schema_mode(SchemaMode::Check);
    assert!(matches!(
        run_until(config, async {}).await,
        Err(ServerError::Schema(_))
    ));
    let present: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public._sqlx_migrations')::text")
            .fetch_one(&pool)
            .await?;
    assert!(present.is_none());
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn check_mode_starts_with_a_no_ddl_read_only_role(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    // SQLx's lossy serializer omits startup options. Append them explicitly and
    // prove the exact URL used by the process selects the restricted role.
    let mut url = pool.connect_options().to_url_lossy();
    url.query_pairs_mut().append_pair(
        "options",
        "-c role=pg_read_all_data -c default_transaction_read_only=on",
    );
    let control = PgPoolOptions::new()
        .max_connections(1)
        .connect(url.as_str())
        .await?;
    let role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&control)
        .await?;
    assert_eq!(role, "pg_read_all_data");
    control.close().await;
    let config = ServerConfig::from_values(url.to_string(), "127.0.0.1:0")?
        .with_schema_mode(SchemaMode::Check);
    tokio::time::timeout(Duration::from_secs(10), run_until(config, async {})).await??;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn shutdown_drains_inflight_http_joins_cleanup_and_closes_the_pool(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let app = Router::new().route(
        "/inflight",
        get({
            let entered = entered.clone();
            let release = release.clone();
            move || {
                let entered = entered.clone();
                let release = release.clone();
                async move {
                    entered.notify_one();
                    release.notified().await;
                    StatusCode::NO_CONTENT
                }
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (stop, stopped) = oneshot::channel();
    let mut tasks = tokio::task::JoinSet::new();
    let server_pool = pool.clone();
    tasks.spawn(async move {
        serve_listener(listener, app, server_pool, true, async move {
            let _ = stopped.await;
        })
        .await
    });
    let request = reqwest::Client::new()
        .get(format!("http://{address}/inflight"))
        .send();
    tokio::pin!(request);
    tokio::select! {
        () = entered.notified() => {},
        result = &mut request => panic!("request finished before its release: {}", result.is_ok()),
        () = tokio::time::sleep(Duration::from_secs(10)) => panic!("request never reached handler"),
    }
    stop.send(()).expect("server owns shutdown receiver");
    release.notify_one();
    assert_eq!(request.await?.status(), StatusCode::NO_CONTENT);
    tokio::time::timeout(Duration::from_secs(10), tasks.join_next())
        .await?
        .expect("server task")??;
    assert!(
        pool.is_closed(),
        "cleanup must release its pool before return"
    );
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    Ok(())
}

#[test]
fn process_errors_do_not_render_database_or_sql_details() {
    let secret = "private-database-diagnostic";
    let error = ServerError::DatabaseConnection(sqlx::Error::Protocol(secret.to_owned()));
    assert!(!format!("{error:?} {error}").contains(secret));
    let migration_connection =
        crate::MigrationCommandError::Connection(sqlx::Error::Protocol(secret.to_owned()));
    assert!(!format!("{migration_connection:?} {migration_connection}").contains(secret));
    assert!(std::error::Error::source(&migration_connection).is_some());
    let migration = SchemaPreparationError::Migration(sqlx::migrate::MigrateError::Execute(
        sqlx::Error::Protocol(secret.to_owned()),
    ));
    assert!(!format!("{migration:?} {migration}").contains(secret));
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn authenticated_runtime_reads_work_under_a_non_ddl_role(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    use crate::{PlatformBootstrapConfig, bootstrap_platform_operator};
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    bootstrap_platform_operator(
        PlatformBootstrapConfig::from_values(
            pool.connect_options().to_url_lossy().to_string(),
            "operator-302",
            "workforce",
            "subject-302",
        )?
        .with_schema_mode(SchemaMode::Check),
    )
    .await?;
    let options = pool.connect_options().as_ref().clone().options([
        ("role", "pg_read_all_data"),
        ("default_transaction_read_only", "on"),
    ]);
    let restricted = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await?;
    let config = ServerConfig::from_values("unused", "127.0.0.1:0")?
        .with_schema_mode(SchemaMode::Check)
        .with_bearer_identity("a".repeat(64), "workforce", "subject-302")?;
    config.schema_mode().prepare(&restricted).await?;
    let router = application_router(restricted.clone(), &config).await?;
    let response = router
        .oneshot(
            Request::get("/api/v1/admin/actors")
                .header("Authorization", format!("Bearer {}", "a".repeat(64)))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    restricted.close().await;
    Ok(())
}
