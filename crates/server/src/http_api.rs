use axum::{
    Json, Router,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Serialize;

const API_VERSION: &str = "v1";
const NOT_FOUND_TYPE: &str = "urn:fleetiq:problem:not-found";
const METHOD_NOT_ALLOWED_TYPE: &str = "urn:fleetiq:problem:method-not-allowed";

/// Builds the unauthenticated version-discovery and API error surface.
///
/// `GET /api/v1` identifies the active major HTTP contract. Unknown resources
/// and unsupported methods return bounded problem-details documents without
/// echoing request data or exposing internal failures. Protected business
/// routes are merged from separately configured adapters.
pub fn api_router() -> Router {
    Router::new()
        .route("/api/v1", get(api_description).fallback(method_not_allowed))
        .fallback(not_found)
}

async fn api_description() -> Json<ApiDescription> {
    Json(ApiDescription {
        service: "FleetIQ",
        version: API_VERSION,
    })
}

async fn not_found() -> ApiProblem {
    ApiProblem::new(StatusCode::NOT_FOUND, NOT_FOUND_TYPE, "Resource not found")
}

pub(crate) async fn method_not_allowed() -> Response {
    let mut response = ApiProblem::new(
        StatusCode::METHOD_NOT_ALLOWED,
        METHOD_NOT_ALLOWED_TYPE,
        "Method not allowed",
    )
    .into_response();
    response
        .headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
    response
}

#[derive(Serialize)]
struct ApiDescription {
    service: &'static str,
    version: &'static str,
}

pub(crate) struct ApiProblem {
    status: StatusCode,
    problem_type: &'static str,
    title: &'static str,
}

impl ApiProblem {
    pub(crate) const fn new(
        status: StatusCode,
        problem_type: &'static str,
        title: &'static str,
    ) -> Self {
        Self {
            status,
            problem_type,
            title,
        }
    }
}

impl IntoResponse for ApiProblem {
    fn into_response(self) -> Response {
        let status = self.status;
        let mut response = (
            status,
            Json(ProblemDetails {
                problem_type: self.problem_type,
                title: self.title,
                status: status.as_u16(),
            }),
        )
            .into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        response
    }
}

#[derive(Serialize)]
struct ProblemDetails {
    #[serde(rename = "type")]
    problem_type: &'static str,
    title: &'static str,
    status: u16,
}
