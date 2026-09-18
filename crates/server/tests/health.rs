//! HTTP health-surface tests at the router boundary.

use std::error::Error;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use fleetiq_server::health_router;
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;

#[tokio::test]
async fn liveness_does_not_depend_on_postgresql() -> Result<(), Box<dyn Error>> {
    let pool = PgPoolOptions::new().connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")?;

    let response = health_router(pool)
        .oneshot(Request::get("/health/live").body(Body::empty())?)
        .await?;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn readiness_tracks_postgresql_availability(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let ready_response = health_router(pool.clone())
        .oneshot(Request::get("/health/ready").body(Body::empty())?)
        .await?;
    assert_eq!(ready_response.status(), StatusCode::NO_CONTENT);

    pool.close().await;
    let unavailable_response = health_router(pool)
        .oneshot(Request::get("/health/ready").body(Body::empty())?)
        .await?;
    assert_eq!(
        unavailable_response.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    Ok(())
}
