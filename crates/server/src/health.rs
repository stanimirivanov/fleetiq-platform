use axum::{Router, extract::State, http::StatusCode, routing::get};
use sqlx::PgPool;

/// Builds the unauthenticated operational health surface.
///
/// Liveness reports that the process can serve HTTP. Readiness additionally
/// requires a successful PostgreSQL round trip. Neither endpoint exposes
/// configuration, dependency errors, tenant data, or database details.
pub fn health_router(pool: PgPool) -> Router {
    Router::new()
        .route("/health/live", get(liveness))
        .route("/health/ready", get(readiness))
        .with_state(pool)
}

async fn liveness() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn readiness(State(pool): State<PgPool>) -> StatusCode {
    match sqlx::query_scalar!(r#"SELECT 1 AS "value!""#)
        .fetch_one(&pool)
        .await
    {
        Ok(1) => StatusCode::NO_CONTENT,
        Ok(_) | Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}
