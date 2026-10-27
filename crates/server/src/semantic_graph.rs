//! Authenticated metadata graph transport; traversal remains application-owned.
mod dto;
#[cfg(test)]
mod tests;
use crate::protected_request::{self, ProtectedRequestError, RequestAuthenticator};
use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        rejection::{PathRejection, QueryRejection},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use fleetiq_access::TenantAuthorizationService;
use fleetiq_domain::{AssetId, TenantId};
use fleetiq_metadata::SemanticGraphService;
use fleetiq_postgres::{PostgresSemanticGraphRepository, PostgresTenantAccessPolicy};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
struct Context<A> {
    authentication: A,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    graph: SemanticGraphService<PostgresSemanticGraphRepository>,
}
pub(crate) fn router<A: RequestAuthenticator + 'static>(pool: PgPool, authentication: A) -> Router {
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/assets/{asset_id}/semantic-graph",
            get(explore::<A>).fallback(protected_request::protected_method_not_allowed),
        )
        .with_state(Arc::new(Context {
            authentication,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
                pool.clone(),
            )),
            graph: SemanticGraphService::new(PostgresSemanticGraphRepository::new(pool)),
        }))
}
async fn explore<A: RequestAuthenticator>(
    State(ctx): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    parameters: Result<Query<dto::Parameters>, QueryRejection>,
) -> Result<Response, ProtectedRequestError> {
    let actor = protected_request::authenticate(&headers, &ctx.authentication).await?;
    let tenant = path
        .as_ref()
        .ok()
        .and_then(|p| TenantId::new(p.0.0.as_str()).ok())
        .ok_or(ProtectedRequestError::Forbidden)?;
    let grant =
        protected_request::authorize_metadata_read(actor, tenant, &ctx.authorization).await?;
    let (Ok(Path((_, root))), Ok(Query(parameters))) = (path, parameters) else {
        return Ok(invalid());
    };
    let Some(query) = AssetId::new(root)
        .ok()
        .and_then(|root| parameters.query(root))
    else {
        return Ok(invalid());
    };
    // Covers all layers and final hydration, not just each individual statement.
    // Dropping a timed-out read closes its read-only transaction; no fake partial success.
    let result = tokio::time::timeout(Duration::from_secs(8), ctx.graph.explore(grant, &query))
        .await
        .map_err(|_| ProtectedRequestError::Internal)?
        .map_err(|_| ProtectedRequestError::Internal)?;
    match result {
        Some(graph) => Ok(protected_request::no_store(
            Json(dto::GraphBody::from(&graph)).into_response(),
        )),
        None => Ok(protected_request::problem(
            StatusCode::NOT_FOUND,
            protected_request::NOT_FOUND,
            "Resource not found",
        )),
    }
}
fn invalid() -> Response {
    protected_request::problem(
        StatusCode::BAD_REQUEST,
        protected_request::BAD_REQUEST,
        "Invalid request",
    )
}
