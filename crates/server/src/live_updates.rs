//! Same-origin, demand-driven SSE replacement delivery for one installed property.
mod cursor;
mod delivery;
mod source;
#[cfg(test)]
mod tests;
use crate::protected_request::{self, ProtectedRequestError, RequestAuthenticator};
use axum::{
    Router,
    extract::{Path, State, rejection::PathRejection},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response, Sse},
    routing::get,
};
use fleetiq_access::{ReadTelemetry, TenantAuthorizationError, TenantAuthorizationService};
use fleetiq_domain::TenantId;
use fleetiq_postgres::{PostgresCurrentStateRepository, PostgresTenantAccessPolicy};
use fleetiq_projection::CurrentStateService;
use source::AuthorizedSource;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;

const IO_TIMEOUT: Duration = Duration::from_secs(8);
const CONNECTION_LIMIT: usize = 64;
type Scope = (String, String, String, String);
struct Context<A> {
    authentication: A,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    current: CurrentStateService<PostgresCurrentStateRepository>,
    capacity: Arc<Semaphore>,
}
pub(crate) fn router<A: RequestAuthenticator + 'static>(pool: PgPool, authentication: A) -> Router {
    routes(Arc::new(Context {
        authentication,
        authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
            pool.clone(),
        )),
        current: CurrentStateService::new(PostgresCurrentStateRepository::new(pool)),
        capacity: Arc::new(Semaphore::new(CONNECTION_LIMIT)),
    }))
}
fn routes<A: RequestAuthenticator + 'static>(context: Arc<Context<A>>) -> Router {
    Router::new().route(
        "/api/v1/tenants/{tenant_id}/assets/{asset_id}/properties/{property_id}/versions/{version}/live",
        get(open::<A>).fallback(protected_request::protected_method_not_allowed),
    ).with_state(context)
}
async fn open<A: RequestAuthenticator + 'static>(
    State(context): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<Scope>, PathRejection>,
) -> Result<Response, ProtectedRequestError> {
    tokio::time::timeout(IO_TIMEOUT, prepare(context, headers, path))
        .await
        .map_err(|_| ProtectedRequestError::Internal)?
}
async fn prepare<A: RequestAuthenticator + 'static>(
    context: Arc<Context<A>>,
    headers: HeaderMap,
    path: Result<Path<Scope>, PathRejection>,
) -> Result<Response, ProtectedRequestError> {
    let actor = protected_request::authenticate(&headers, &context.authentication).await?;
    let tenant = path
        .as_ref()
        .ok()
        .and_then(|p| TenantId::new(p.0.0.as_str()).ok())
        .ok_or(ProtectedRequestError::Forbidden)?;
    let actor_id = actor.actor_id().clone();
    let grant = context
        .authorization
        .authorize::<ReadTelemetry>(actor, tenant.clone())
        .await
        .map_err(|e| match e {
            TenantAuthorizationError::Denied => ProtectedRequestError::Forbidden,
            TenantAuthorizationError::Policy(_) => ProtectedRequestError::Internal,
        })?;
    let Ok(Path((_, asset, property, version))) = path else {
        return Ok(invalid());
    };
    let Some(target) = crate::current_state::target(asset, property, &version) else {
        return Ok(invalid());
    };
    let scope = cursor::scope(&tenant, &target);
    let mut values = headers.get_all("last-event-id").iter();
    let previous = match values.next() {
        None => None,
        Some(value) => {
            let Some(position) = value.to_str().ok().and_then(|v| cursor::decode(&scope, v)) else {
                return Ok(invalid());
            };
            Some(position)
        }
    };
    if values.next().is_some() {
        return Ok(invalid());
    }
    let Ok(permit) = context.capacity.clone().try_acquire_owned() else {
        let mut response = protected_request::problem(
            StatusCode::TOO_MANY_REQUESTS,
            "urn:fleetiq:problem:live-capacity",
            "Live connection capacity reached",
        );
        response
            .headers_mut()
            .insert("retry-after", HeaderValue::from_static("2"));
        return Ok(response);
    };
    if context
        .current
        .snapshot(grant, &target)
        .await
        .map_err(|_| ProtectedRequestError::Internal)?
        .is_none()
    {
        return Ok(protected_request::problem(
            StatusCode::NOT_FOUND,
            protected_request::NOT_FOUND,
            "Resource not found",
        ));
    }
    // Recheck authority/data on first body demand: headers may wait in a proxy or
    // application before polling. Do not retain a preauthorized first payload.
    let source = AuthorizedSource {
        context,
        headers,
        tenant,
        target,
        actor_id,
    };
    let stream = delivery::stream(source, scope, previous, permit);
    let mut response = protected_request::no_store(Sse::new(stream).into_response());
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(response)
}
fn invalid() -> Response {
    protected_request::problem(
        StatusCode::BAD_REQUEST,
        protected_request::BAD_REQUEST,
        "Invalid request",
    )
}
