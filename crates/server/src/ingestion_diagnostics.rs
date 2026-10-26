//! Authorized safe operational evidence; no payload inspection or inferred liveness.
mod dto;
#[cfg(test)]
mod tests;
use crate::protected_request::{self, ProtectedRequestError, RequestAuthenticator};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use fleetiq_access::{ManageProtocols, TenantAuthorizationError, TenantAuthorizationService};
use fleetiq_domain::{AssetId, TenantId};
use fleetiq_ingestion::IngestionDiagnosticsService;
use fleetiq_postgres::{PostgresIngestionDiagnostics, PostgresTenantAccessPolicy};
use sqlx::PgPool;
use std::sync::Arc;
struct Context<A> {
    authentication: A,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    diagnostics: IngestionDiagnosticsService<PostgresIngestionDiagnostics>,
}
pub(crate) fn router<A: RequestAuthenticator + 'static>(pool: PgPool, authentication: A) -> Router {
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/assets/{asset_id}/ingestion-diagnostics",
            get(find::<A>).fallback(protected_request::resource_method_not_allowed),
        )
        .with_state(Arc::new(Context {
            authentication,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
                pool.clone(),
            )),
            diagnostics: IngestionDiagnosticsService::new(PostgresIngestionDiagnostics::new(pool)),
        }))
}
async fn find<A: RequestAuthenticator>(
    State(context): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, axum::extract::rejection::PathRejection>,
) -> Result<Response, ProtectedRequestError> {
    let actor = protected_request::authenticate(&headers, &context.authentication).await?;
    let tenant = path
        .as_ref()
        .ok()
        .and_then(|p| TenantId::new(p.0.0.as_str()).ok())
        .ok_or(ProtectedRequestError::Forbidden)?;
    let authorization = context
        .authorization
        .authorize::<ManageProtocols>(actor, tenant)
        .await
        .map_err(|e| match e {
            TenantAuthorizationError::Denied => ProtectedRequestError::Forbidden,
            TenantAuthorizationError::Policy(_) => ProtectedRequestError::Internal,
        })?;
    let device = path.ok().and_then(|p| AssetId::new(p.0.1).ok());
    let Some(device) = device else {
        return Ok(protected_request::problem(
            StatusCode::BAD_REQUEST,
            protected_request::BAD_REQUEST,
            "Invalid request",
        ));
    };
    match context
        .diagnostics
        .find(authorization, &device)
        .await
        .map_err(|_| ProtectedRequestError::Internal)?
    {
        Some(snapshot) => Ok(protected_request::no_store(
            Json(dto::SnapshotBody::from(snapshot)).into_response(),
        )),
        None => Ok(protected_request::problem(
            StatusCode::NOT_FOUND,
            protected_request::NOT_FOUND,
            "Resource not found",
        )),
    }
}
