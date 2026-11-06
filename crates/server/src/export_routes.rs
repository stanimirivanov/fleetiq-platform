//! Protected export transport; durable workers and downloads have independent global bounds.
mod delivery;
mod dto;
#[cfg(test)]
mod tests;
use crate::{
    mutation_guard::MutationGuard,
    protected_request::{self, ProtectedRequestError, RequestAuthenticator},
};
use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, Query, State,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, put},
};
use fleetiq_access::{
    AuthorizedTenant, ManageOperations, ReadTelemetry, TenantAuthorizationError,
    TenantAuthorizationService,
};
use fleetiq_domain::TenantId;
use fleetiq_history::exports::{ExportFailure, ExportKey, ExportTicket};
use fleetiq_postgres::{PostgresExports, PostgresTenantAccessPolicy};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Semaphore;
struct Context<A> {
    authentication: A,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    mutation: MutationGuard,
    pool: PgPool,
    store: PostgresExports,
    workers: Arc<Semaphore>,
    downloads: Arc<Semaphore>,
}
pub(crate) fn router<A: RequestAuthenticator + 'static>(
    pool: PgPool,
    authentication: A,
    mutation: MutationGuard,
) -> Router {
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/exports/requests/{request_key}",
            put(create::<A>).fallback(protected_request::resource_method_not_allowed),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/exports/jobs/{export_id}",
            get(status::<A>)
                .delete(cancel::<A>)
                .fallback(protected_request::resource_method_not_allowed),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/exports/jobs/{export_id}/download",
            get(delivery::download::<A>)
                .head(delivery::head::<A>)
                .fallback(protected_request::resource_method_not_allowed),
        )
        .layer(DefaultBodyLimit::max(4096))
        .with_state(Arc::new(Context {
            authentication,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
                pool.clone(),
            )),
            mutation,
            store: PostgresExports::new(pool.clone()),
            pool,
            workers: Arc::new(Semaphore::new(8)),
            downloads: Arc::new(Semaphore::new(8)),
        }))
}
type Scope = (String, String);
fn problem(status: StatusCode, title: &'static str) -> Response {
    protected_request::problem(status, "urn:fleetiq:problem:historical-export", title)
}
fn policy(
    e: &TenantAuthorizationError<fleetiq_postgres::PostgresTenantAccessPolicyError>,
) -> ProtectedRequestError {
    match e {
        TenantAuthorizationError::Denied => ProtectedRequestError::Forbidden,
        TenantAuthorizationError::Policy(_) => ProtectedRequestError::Internal,
    }
}
async fn authorize<A: RequestAuthenticator>(
    ctx: &Context<A>,
    headers: &HeaderMap,
    tenant: &str,
) -> Result<AuthorizedTenant<ReadTelemetry>, ProtectedRequestError> {
    let actor = protected_request::authenticate(headers, &ctx.authentication).await?;
    let tenant = TenantId::new(tenant).map_err(|_| ProtectedRequestError::Forbidden)?;
    ctx.authorization
        .authorize::<ReadTelemetry>(actor, tenant)
        .await
        .map_err(|e| policy(&e))
}
async fn management<A: RequestAuthenticator>(
    ctx: &Context<A>,
    headers: &HeaderMap,
    read: &AuthorizedTenant<ReadTelemetry>,
) -> Result<AuthorizedTenant<ManageOperations>, ProtectedRequestError> {
    let actor = protected_request::authenticate(headers, &ctx.authentication).await?;
    if actor.actor_id() != read.actor_id() {
        return Err(ProtectedRequestError::Forbidden);
    }
    let grant = ctx
        .authorization
        .authorize::<ManageOperations>(actor, read.tenant_id().clone())
        .await
        .map_err(|e| policy(&e))?;
    ctx.mutation.check(headers)?;
    Ok(grant)
}

fn failure(e: ExportFailure) -> Response {
    match e {
        ExportFailure::Bounds => problem(StatusCode::BAD_REQUEST, "Invalid export request"),
        ExportFailure::Limit => problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Export selection exceeds supported limits",
        ),
        ExportFailure::Coverage => problem(StatusCode::CONFLICT, "Export coverage unavailable"),
        ExportFailure::Conflict => {
            problem(StatusCode::CONFLICT, "Export request identity conflicts")
        }
        ExportFailure::Disabled | ExportFailure::Denied => {
            problem(StatusCode::TOO_MANY_REQUESTS, "Export work unavailable")
        }
        ExportFailure::Storage => ProtectedRequestError::Internal.into_response(),
    }
}
fn ticket(raw: &str) -> Option<ExportTicket> {
    ExportTicket::new(dto::integer(raw)?).ok()
}
async fn create<A: RequestAuthenticator + 'static>(
    State(ctx): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<Scope>, PathRejection>,
    query: Result<Query<dto::Empty>, QueryRejection>,
    body: Result<Json<dto::Create>, JsonRejection>,
) -> Result<Response, ProtectedRequestError> {
    let read = authorize(&ctx, &headers, path.as_ref().map_or("", |p| p.0.0.as_str())).await?;
    let tenant = read.tenant_id().clone();
    let manage = management(&ctx, &headers, &read).await?;
    let (Ok(Path((_, key))), Ok(Query(_))) = (path, query) else {
        return Ok(problem(StatusCode::BAD_REQUEST, "Invalid request"));
    };
    let body = match body {
        Ok(Json(body)) => body,
        Err(e) => {
            return Ok(problem(
                if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
                    StatusCode::PAYLOAD_TOO_LARGE
                } else {
                    StatusCode::BAD_REQUEST
                },
                "Invalid request",
            ));
        }
    };
    let (Ok(key), Some(request)) = (ExportKey::new(key), body.request(tenant.clone())) else {
        return Ok(problem(StatusCode::BAD_REQUEST, "Invalid export request"));
    };
    let Ok(permit) = ctx.workers.clone().try_acquire_owned() else {
        return Ok(problem(
            StatusCode::TOO_MANY_REQUESTS,
            "Export workers busy",
        ));
    };
    let ticket = match tokio::time::timeout(
        std::time::Duration::from_secs(20),
        ctx.store.request_authorized(
            read,
            manage,
            &key,
            &request,
            &fleetiq_archive::ExactExportCodec,
        ),
    )
    .await
    .map_err(|_| ProtectedRequestError::Internal)?
    {
        Ok(t) => t,
        Err(e) => return Ok(failure(e)),
    };
    let Some(info) = ctx
        .store
        .describe(&tenant, ticket)
        .await
        .map_err(|_| ProtectedRequestError::Internal)?
    else {
        return Err(ProtectedRequestError::Internal);
    };
    if info.status == fleetiq_history::exports::ExportStatus::Pending {
        let pool = ctx.pool.clone();
        tokio::spawn(async move {
            let _permit = permit;
            if let Err(e) = crate::export_once(pool, &tenant, ticket).await {
                eprintln!("export worker: {e}");
            }
        });
    }
    Ok(protected_request::no_store(
        (StatusCode::ACCEPTED, Json(dto::job(&info))).into_response(),
    ))
}
async fn status<A: RequestAuthenticator>(
    State(ctx): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<Scope>, PathRejection>,
    query: Result<Query<dto::Empty>, QueryRejection>,
) -> Result<Response, ProtectedRequestError> {
    inspect(ctx, headers, path, query, false).await
}
async fn cancel<A: RequestAuthenticator>(
    State(ctx): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<Scope>, PathRejection>,
    query: Result<Query<dto::Empty>, QueryRejection>,
) -> Result<Response, ProtectedRequestError> {
    inspect(ctx, headers, path, query, true).await
}
async fn inspect<A: RequestAuthenticator>(
    ctx: Arc<Context<A>>,
    headers: HeaderMap,
    path: Result<Path<Scope>, PathRejection>,
    query: Result<Query<dto::Empty>, QueryRejection>,
    cancel: bool,
) -> Result<Response, ProtectedRequestError> {
    let read = authorize(&ctx, &headers, path.as_ref().map_or("", |p| p.0.0.as_str())).await?;
    let tenant = read.tenant_id().clone();
    let manage = if cancel {
        Some(management(&ctx, &headers, &read).await?)
    } else {
        None
    };
    let (Ok(Path((_, id))), Ok(Query(_))) = (path, query) else {
        return Ok(problem(StatusCode::BAD_REQUEST, "Invalid request"));
    };
    let Some(ticket) = ticket(&id) else {
        return Ok(problem(StatusCode::BAD_REQUEST, "Invalid export identity"));
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let info = if let Some(manage) = manage {
            if ctx
                .store
                .describe(&tenant, ticket)
                .await
                .map_err(|_| ProtectedRequestError::Internal)?
                .is_none()
            {
                return Ok(problem(StatusCode::NOT_FOUND, "Resource not found"));
            }
            ctx.store
                .cancel_authorized(read, manage, ticket)
                .await
                .map_err(|_| ProtectedRequestError::Internal)?;
            ctx.store.describe(&tenant, ticket).await
        } else {
            ctx.store.describe_authorized(read, ticket).await
        }
        .map_err(|_| ProtectedRequestError::Internal)?;
        let Some(info) = info else {
            return Ok(problem(StatusCode::NOT_FOUND, "Resource not found"));
        };
        Ok(protected_request::no_store(
            Json(dto::job(&info)).into_response(),
        ))
    })
    .await
    .map_err(|_| ProtectedRequestError::Internal)?
}
