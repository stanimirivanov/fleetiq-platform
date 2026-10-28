//! Protected exact-property snapshots; no projection writes or metadata resolution.
pub(crate) mod dto;
#[cfg(test)]
mod tests;
use crate::protected_request::{self, ProtectedRequestError, RequestAuthenticator};
use axum::{
    Json, Router,
    extract::{Path, State, rejection::PathRejection},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use fleetiq_access::{ReadTelemetry, TenantAuthorizationError, TenantAuthorizationService};
use fleetiq_domain::{
    AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, SignalTarget,
    TenantId, UnixTimeMillis,
};
use fleetiq_postgres::{PostgresCurrentStateRepository, PostgresTenantAccessPolicy};
use fleetiq_projection::CurrentStateService;
use sqlx::PgPool;
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
struct Context<A> {
    authentication: A,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    current: CurrentStateService<PostgresCurrentStateRepository>,
}
type Scope = (String, String, String, String);
pub(crate) fn router<A: RequestAuthenticator + 'static>(pool: PgPool, authentication: A) -> Router {
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/assets/{asset_id}/properties/{property_id}/versions/{version}/current",
            get(snapshot::<A>).fallback(protected_request::protected_method_not_allowed),
        )
        .with_state(Arc::new(Context {
            authentication,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone())),
            current: CurrentStateService::new(PostgresCurrentStateRepository::new(pool)),
        }))
}
async fn snapshot<A: RequestAuthenticator>(
    State(ctx): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<Scope>, PathRejection>,
) -> Result<Response, ProtectedRequestError> {
    let actor = protected_request::authenticate(&headers, &ctx.authentication).await?;
    let tenant = path
        .as_ref()
        .ok()
        .and_then(|p| TenantId::new(p.0.0.as_str()).ok())
        .ok_or(ProtectedRequestError::Forbidden)?;
    let grant = ctx
        .authorization
        .authorize::<ReadTelemetry>(actor, tenant)
        .await
        .map_err(|e| match e {
            TenantAuthorizationError::Denied => ProtectedRequestError::Forbidden,
            TenantAuthorizationError::Policy(_) => ProtectedRequestError::Internal,
        })?;
    let Ok(Path((_, asset, property, version))) = path else {
        return Ok(invalid());
    };
    let Some(target) = target(asset, property, &version) else {
        return Ok(invalid());
    };
    let result = tokio::time::timeout(Duration::from_secs(8), ctx.current.snapshot(grant, &target))
        .await
        .map_err(|_| ProtectedRequestError::Internal)?
        .map_err(|_| ProtectedRequestError::Internal)?;
    let Some(state) = result else {
        return Ok(protected_request::problem(
            StatusCode::NOT_FOUND,
            protected_request::NOT_FOUND,
            "Resource not found",
        ));
    };
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|t| i64::try_from(t.as_millis()).ok())
        .ok_or(ProtectedRequestError::Internal)?;
    let body = dto::Body::new(&state, UnixTimeMillis::new(millis))
        .map_err(|_| ProtectedRequestError::Internal)?;
    Ok(protected_request::no_store(Json(body).into_response()))
}
pub(crate) fn target(asset: String, property: String, version: &str) -> Option<SignalTarget> {
    if version.is_empty() || !version.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(SignalTarget::new(
        AssetId::new(asset).ok()?,
        PropertyDefinitionRef::new(
            PropertyDefinitionId::new(property).ok()?,
            DefinitionVersion::new(version.parse().ok()?).ok()?,
        ),
    ))
}
fn invalid() -> Response {
    protected_request::problem(
        StatusCode::BAD_REQUEST,
        protected_request::BAD_REQUEST,
        "Invalid request",
    )
}
