use axum::{Router, extract::State, http::StatusCode, routing::get};
use sqlx::PgPool;

/// Builds the unauthenticated operational health surface.
///
/// Liveness reports that the process can serve HTTP. Readiness additionally
/// requires an exact successful migration version/checksum snapshot. Neither endpoint exposes
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
    match fleetiq_postgres::check_schema(&pool).await {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}
