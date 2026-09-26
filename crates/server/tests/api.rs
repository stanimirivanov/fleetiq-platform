//! HTTP API compatibility-boundary tests.

use std::error::Error;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use fleetiq_server::{api_router, health_router};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

const MAX_RESPONSE_BYTES: usize = 1_024;

#[tokio::test]
async fn version_root_identifies_the_active_contract() -> Result<(), Box<dyn Error>> {
    let response = api_router()
        .oneshot(Request::get("/api/v1").body(Body::empty())?)
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE),
        Some(&header::HeaderValue::from_static("application/json"))
    );
    assert_eq!(
        to_bytes(response.into_body(), MAX_RESPONSE_BYTES).await?,
        r#"{"service":"FleetIQ","version":"v1"}"#
    );
    Ok(())
}

#[tokio::test]
async fn unknown_resources_return_bounded_problem_details() -> Result<(), Box<dyn Error>> {
    let response = api_router()
        .oneshot(Request::get("/api/v1/unknown").body(Body::empty())?)
        .await?;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE),
        Some(&header::HeaderValue::from_static(
            "application/problem+json"
        ))
    );
    assert_eq!(
        to_bytes(response.into_body(), MAX_RESPONSE_BYTES).await?,
        r#"{"type":"urn:fleetiq:problem:not-found","title":"Resource not found","status":404}"#
    );
    Ok(())
}

#[tokio::test]
async fn unsupported_methods_name_the_allowed_method() -> Result<(), Box<dyn Error>> {
    let response = api_router()
        .oneshot(Request::post("/api/v1").body(Body::empty())?)
        .await?;

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        response.headers().get(header::ALLOW),
        Some(&header::HeaderValue::from_static("GET, HEAD"))
    );
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE),
        Some(&header::HeaderValue::from_static(
            "application/problem+json"
        ))
    );
    assert_eq!(
        to_bytes(response.into_body(), MAX_RESPONSE_BYTES).await?,
        r#"{"type":"urn:fleetiq:problem:method-not-allowed","title":"Method not allowed","status":405}"#
    );
    Ok(())
}

#[tokio::test]
async fn api_fallback_preserves_the_operational_health_surface() -> Result<(), Box<dyn Error>> {
    let pool = PgPoolOptions::new().connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")?;
    let application = Router::new().merge(health_router(pool)).merge(api_router());

    let response = application
        .oneshot(Request::get("/health/live").body(Body::empty())?)
        .await?;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        to_bytes(response.into_body(), MAX_RESPONSE_BYTES).await?,
        ""
    );
    Ok(())
}
