//! Protected fleet read transport; coordinate meaning and coherence stay inward.
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
use fleetiq_access::{ReadTelemetry, TenantAuthorizationError, TenantAuthorizationService};
use fleetiq_domain::TenantId;
use fleetiq_postgres::{PostgresCurrentStateRepository, PostgresTenantAccessPolicy};
use fleetiq_projection::{FleetReadError, FleetReadService};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
struct Context<A> {
    authentication: A,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    fleet: FleetReadService<PostgresCurrentStateRepository>,
}
pub(crate) fn router<A: RequestAuthenticator + 'static>(pool: PgPool, authentication: A) -> Router {
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/fleet",
            get(snapshot::<A>).fallback(protected_request::protected_method_not_allowed),
        )
        .with_state(Arc::new(Context {
            authentication,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
                pool.clone(),
            )),
            fleet: FleetReadService::new(PostgresCurrentStateRepository::new(pool)),
        }))
}
async fn snapshot<A: RequestAuthenticator>(
    State(ctx): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    parameters: Result<Query<dto::Parameters>, QueryRejection>,
) -> Result<Response, ProtectedRequestError> {
    let actor = protected_request::authenticate(&headers, &ctx.authentication).await?;
    let tenant = path
        .as_ref()
        .ok()
        .and_then(|p| TenantId::new(p.0.as_str()).ok())
        .ok_or(ProtectedRequestError::Forbidden)?;
    let metadata =
        protected_request::authorize_metadata_read(actor, tenant.clone(), &ctx.authorization)
            .await?;
    // Authentication evidence is deliberately consumable, not Clone. Reauthenticate
    // for the second capability and let the service reject mapping/account changes.
    let actor = protected_request::authenticate(&headers, &ctx.authentication).await?;
    let telemetry = ctx
        .authorization
        .authorize::<ReadTelemetry>(actor, tenant)
        .await
        .map_err(|e| match e {
            TenantAuthorizationError::Denied => ProtectedRequestError::Forbidden,
            TenantAuthorizationError::Policy(_) => ProtectedRequestError::Internal,
        })?;
    let Ok(Query(parameters)) = parameters else {
        return Ok(invalid());
    };
    let Some(query) = parameters.query() else {
        return Ok(invalid());
    };
    let result = tokio::time::timeout(
        Duration::from_secs(8),
        ctx.fleet.read(metadata, telemetry, &query),
    )
    .await
    .map_err(|_| ProtectedRequestError::Internal)?
    .map_err(|e| match e {
        FleetReadError::ScopeMismatch => ProtectedRequestError::Forbidden,
        FleetReadError::Repository(_) => ProtectedRequestError::Internal,
    })?;
    let Some(snapshot) = result else {
        return Ok(protected_request::problem(
            StatusCode::NOT_FOUND,
            protected_request::NOT_FOUND,
            "Resource not found",
        ));
    };
    let body =
        dto::Body::new(&snapshot, query.mapping()).map_err(|_| ProtectedRequestError::Internal)?;
    Ok(protected_request::no_store(Json(body).into_response()))
}
fn invalid() -> Response {
    protected_request::problem(
        StatusCode::BAD_REQUEST,
        protected_request::BAD_REQUEST,
        "Invalid request",
    )
}
