//! Generation-bound requests for temporary verified archive staging.
#[cfg(test)]
mod tests;

use crate::{
    mutation_guard::MutationGuard,
    protected_request::{self, ProtectedRequestError, RequestAuthenticator},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{
        DefaultBodyLimit, Path, Query, State,
        rejection::{BytesRejection, PathRejection, QueryRejection},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, put},
};
use fleetiq_access::{
    AuthorizedTenant, ManageOperations, ReadTelemetry, TenantActionMarker,
    TenantAuthorizationError, TenantAuthorizationService,
};
use fleetiq_archive::objects::S3ArchiveConfig;
use fleetiq_domain::TenantId;
use fleetiq_history::restore::{RestoreStatus, RestoreTicket};
use fleetiq_postgres::{PostgresRestore, PostgresTenantAccessPolicy, RestoreRequestError};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;

struct Context<A> {
    authentication: A,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    mutation: MutationGuard,
    pool: PgPool,
    store: PostgresRestore,
    archive: Option<S3ArchiveConfig>,
    attempts: Arc<Semaphore>,
}

pub(crate) fn router<A: RequestAuthenticator + 'static>(
    pool: PgPool,
    authentication: A,
    mutation: MutationGuard,
    archive: Option<S3ArchiveConfig>,
) -> Router {
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/restores/manifests/{manifest_id}/requests/{expected_generation}",
            put(request::<A>).fallback(protected_request::resource_method_not_allowed),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/restores/manifests/{manifest_id}/generations/{restore_generation}",
            get(status::<A>).fallback(protected_request::resource_method_not_allowed),
        )
        .layer(DefaultBodyLimit::max(4096))
        .with_state(Arc::new(Context {
            authentication,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone())),
            mutation,
            store: PostgresRestore::new(pool.clone()),
            pool,
            archive,
            attempts: Arc::new(Semaphore::new(8)),
        }))
}

type Scope = (String, String, String);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyQuery {}

async fn authorize<A: RequestAuthenticator, M: TenantActionMarker>(
    ctx: &Context<A>,
    headers: &HeaderMap,
    tenant: &str,
) -> Result<AuthorizedTenant<M>, ProtectedRequestError> {
    let actor = protected_request::authenticate(headers, &ctx.authentication).await?;
    let tenant = TenantId::new(tenant).map_err(|_| ProtectedRequestError::Forbidden)?;
    ctx.authorization
        .authorize::<M>(actor, tenant)
        .await
        .map_err(|error| match error {
            TenantAuthorizationError::Denied => ProtectedRequestError::Forbidden,
            TenantAuthorizationError::Policy(_) => ProtectedRequestError::Internal,
        })
}

fn integer(raw: &str) -> Option<i64> {
    let parsed = raw.parse::<i64>().ok()?;
    (parsed.to_string() == raw).then_some(parsed)
}

fn problem(status: StatusCode, title: &'static str) -> Response {
    protected_request::problem(status, "urn:fleetiq:problem:archive-restore", title)
}

fn invalid() -> Response {
    problem(StatusCode::BAD_REQUEST, "Invalid restore request")
}

fn receipt(code: StatusCode, ticket: RestoreTicket, state: &RestoreStatus) -> Response {
    let state = match state {
        RestoreStatus::Missing => {
            return problem(StatusCode::NOT_FOUND, "Restore generation not found");
        }
        RestoreStatus::Pending => "pending",
        RestoreStatus::Ready => "ready",
        RestoreStatus::Cancelled => "cancelled",
        RestoreStatus::Expired => "expired",
    };
    protected_request::no_store(
        (
            code,
            Json(json!({
                "manifest_id": ticket.manifest().to_string(),
                "restore_generation": ticket.generation().to_string(),
                "state": state,
            })),
        )
            .into_response(),
    )
}

async fn request<A: RequestAuthenticator + 'static>(
    State(ctx): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<Scope>, PathRejection>,
    query: Result<Query<EmptyQuery>, QueryRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Result<Response, ProtectedRequestError> {
    let tenant = path.as_ref().map_or("", |p| p.0.0.as_str());
    let read = authorize::<_, ReadTelemetry>(&ctx, &headers, tenant).await?;
    let manage = authorize::<_, ManageOperations>(&ctx, &headers, tenant).await?;
    if manage.actor_id() != read.actor_id() {
        return Err(ProtectedRequestError::Forbidden);
    }
    ctx.mutation.check(&headers)?;
    let (Ok(Path((_, manifest, generation))), Ok(Query(_)), Ok(body)) = (path, query, body) else {
        return Ok(invalid());
    };
    let (Some(manifest), Some(expected)) = (integer(&manifest), integer(&generation)) else {
        return Ok(invalid());
    };
    if manifest <= 0 || !(0..i64::MAX).contains(&expected) || !body.is_empty() {
        return Ok(invalid());
    }
    let Some(archive) = ctx.archive.clone() else {
        return Ok(problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "Archive storage is unavailable",
        ));
    };
    let Ok(permit) = ctx.attempts.clone().try_acquire_owned() else {
        return Ok(problem(
            StatusCode::TOO_MANY_REQUESTS,
            "Restore workers busy",
        ));
    };
    let tenant = read.tenant_id().clone();
    let ticket = match tokio::time::timeout(
        Duration::from_secs(5),
        ctx.store
            .request_authorized(read, manage, manifest, expected),
    )
    .await
    .map_err(|_| ProtectedRequestError::Internal)?
    {
        Ok(ticket) => ticket,
        Err(RestoreRequestError::Conflict) => {
            return Ok(problem(
                StatusCode::CONFLICT,
                "Restore generation conflicts",
            ));
        }
        Err(RestoreRequestError::Unavailable) => {
            return Ok(problem(
                StatusCode::NOT_FOUND,
                "Restore manifest unavailable",
            ));
        }
        Err(RestoreRequestError::Storage) => return Err(ProtectedRequestError::Internal),
    };
    let state = describe(&ctx, &tenant, ticket).await?;
    if state == RestoreStatus::Pending {
        let pool = ctx.pool.clone();
        tokio::spawn(async move {
            let _permit = permit;
            if crate::restore_once(pool, archive, &tenant, &ticket)
                .await
                .is_err()
            {
                eprintln!("archive restore attempt failed");
            }
        });
    }
    Ok(receipt(StatusCode::ACCEPTED, ticket, &state))
}

async fn status<A: RequestAuthenticator + 'static>(
    State(ctx): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<Scope>, PathRejection>,
    query: Result<Query<EmptyQuery>, QueryRejection>,
) -> Result<Response, ProtectedRequestError> {
    let tenant = path.as_ref().map_or("", |p| p.0.0.as_str());
    let read = authorize::<_, ReadTelemetry>(&ctx, &headers, tenant).await?;
    let (Ok(Path((_, manifest, generation))), Ok(Query(_))) = (path, query) else {
        return Ok(invalid());
    };
    let (Some(manifest), Some(generation)) = (integer(&manifest), integer(&generation)) else {
        return Ok(invalid());
    };
    let Ok(ticket) = RestoreTicket::new(manifest, generation) else {
        return Ok(invalid());
    };
    let state = describe(&ctx, read.tenant_id(), ticket).await?;
    Ok(receipt(StatusCode::OK, ticket, &state))
}

async fn describe<A>(
    ctx: &Context<A>,
    tenant: &TenantId,
    ticket: RestoreTicket,
) -> Result<RestoreStatus, ProtectedRequestError> {
    tokio::time::timeout(Duration::from_secs(5), ctx.store.status(tenant, &ticket))
        .await
        .map_err(|_| ProtectedRequestError::Internal)?
        .map_err(|_| ProtectedRequestError::Internal)
}
