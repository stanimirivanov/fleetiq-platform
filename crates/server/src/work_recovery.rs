//! Explicit operational commands. Retry never rewrites source or completes work.
mod dto;
#[cfg(test)]
pub(crate) mod tests;
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
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, put},
};
use fleetiq_access::{
    AuthorizedTenant, ManageOperations, ReadOperations, TenantAuthorizationError,
    TenantAuthorizationService,
};
use fleetiq_analytics::WorkRecoveryId;
use fleetiq_domain::TenantId;
use fleetiq_postgres::{PostgresTenantAccessPolicy, PostgresWorkRecovery, WorkRecoveryError};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
struct Context<A> {
    authentication: A,
    mutation: MutationGuard,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    store: PostgresWorkRecovery,
}
pub(crate) fn router<A: RequestAuthenticator + 'static>(
    pool: PgPool,
    authentication: A,
    mutation: MutationGuard,
) -> Router {
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/work-recovery/{kind}",
            get(inspect::<A>).fallback(protected_request::resource_method_not_allowed),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/work-recovery/{kind}/commands/{request_id}",
            put(apply::<A>).fallback(command_method_not_allowed),
        )
        .layer(DefaultBodyLimit::max(2048))
        .with_state(Arc::new(Context {
            authentication,
            mutation,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
                pool.clone(),
            )),
            store: PostgresWorkRecovery::new(pool),
        }))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
fn problem(status: StatusCode, title: &'static str) -> Response {
    protected_request::problem(status, "urn:fleetiq:problem:work-recovery", title)
}
fn error(e: &WorkRecoveryError) -> Response {
    match e {
        WorkRecoveryError::Forbidden => ProtectedRequestError::Forbidden.into_response(),
        WorkRecoveryError::Conflict => problem(
            StatusCode::CONFLICT,
            "Work recovery conflicts with current state or request identity",
        ),
        WorkRecoveryError::Storage(_) | WorkRecoveryError::InvalidReceipt => {
            ProtectedRequestError::Internal.into_response()
        }
    }
}
async fn command_method_not_allowed() -> Response {
    let mut r = problem(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed");
    r.headers_mut()
        .insert(header::ALLOW, "PUT".parse().expect("static header"));
    r
}
fn policy(
    e: &TenantAuthorizationError<fleetiq_postgres::PostgresTenantAccessPolicyError>,
) -> ProtectedRequestError {
    match e {
        TenantAuthorizationError::Denied => ProtectedRequestError::Forbidden,
        TenantAuthorizationError::Policy(_) => ProtectedRequestError::Internal,
    }
}
fn tenant(raw: Option<&str>) -> Result<TenantId, ProtectedRequestError> {
    raw.and_then(|tenant_id| TenantId::new(tenant_id).ok())
        .ok_or(ProtectedRequestError::Forbidden)
}
fn kind(work_kind: &str) -> bool {
    matches!(
        work_kind,
        "replay" | "calculator" | "alert_evaluation" | "notification"
    )
}
async fn inspect<A: RequestAuthenticator>(
    State(context): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    query: Result<Query<Empty>, QueryRejection>,
) -> Result<Response, ProtectedRequestError> {
    let actor = protected_request::authenticate(&headers, &context.authentication).await?;
    let tenant_id = tenant(path.as_ref().ok().map(|p| p.0.0.as_str()))?;
    let manage = context
        .authorization
        .authorize::<ManageOperations>(actor, tenant_id.clone())
        .await
        .map_err(|e| policy(&e))?;
    let actor = protected_request::authenticate(&headers, &context.authentication).await?;
    let read = context
        .authorization
        .authorize::<ReadOperations>(actor, tenant_id)
        .await
        .map_err(|e| policy(&e))?;
    let (Ok(Path((_, work_kind))), Ok(Query(_))) = (path, query) else {
        return Ok(problem(StatusCode::BAD_REQUEST, "Invalid recovery query"));
    };
    if !kind(&work_kind) {
        return Ok(problem(StatusCode::BAD_REQUEST, "Invalid work kind"));
    }
    match tokio::time::timeout(
        Duration::from_secs(5),
        context.store.inspect(manage, read, &work_kind),
    )
    .await
    {
        Ok(Ok(view)) => {
            let targets: Vec<_> = view
                .targets
                .into_iter()
                .map(|(target, generation)| json!({"target":target,"expected_generation":generation.to_string()}))
                .collect();
            Ok(protected_request::no_store(Json(json!({"kind":work_kind,"control":{"generation":view.generation.to_string(),"paused":view.paused},"targets":targets,"truncated":view.truncated})).into_response()))
        }
        Ok(Err(e)) => Ok(error(&e)),
        Err(_) => Err(ProtectedRequestError::Internal),
    }
}
async fn apply<A: RequestAuthenticator>(
    State(context): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String, String)>, PathRejection>,
    query: Result<Query<Empty>, QueryRejection>,
    body: Result<Json<dto::Command>, JsonRejection>,
) -> Result<Response, ProtectedRequestError> {
    let actor = protected_request::authenticate(&headers, &context.authentication).await?;
    context.mutation.check(&headers)?;
    let tenant_id = tenant(path.as_ref().ok().map(|p| p.0.0.as_str()))?;
    let auth: AuthorizedTenant<ManageOperations> = context
        .authorization
        .authorize(actor, tenant_id)
        .await
        .map_err(|e| policy(&e))?;
    let (Ok(Path((_, work_kind, request_id))), Ok(Query(_)), Ok(Json(command))) =
        (path, query, body)
    else {
        return Ok(problem(StatusCode::BAD_REQUEST, "Invalid recovery command"));
    };
    let Some((request_id, generation)) = WorkRecoveryId::new(request_id)
        .ok()
        .zip(command.validate(&work_kind))
    else {
        return Ok(problem(
            StatusCode::BAD_REQUEST,
            "Invalid recovery identity or generation",
        ));
    };
    match tokio::time::timeout(Duration::from_secs(6),context.store.apply(auth,&request_id,&work_kind,command.operation.name(),&command.target,generation)).await {
        Ok(Ok(receipt))=>Ok(protected_request::no_store(Json(json!({"request_id":request_id.as_str(),"generation":receipt.generation.to_string(),"actor_id":receipt.actor.as_str(),"recorded_at_ms":receipt.recorded_at.get().to_string()})).into_response())),
        Ok(Err(e))=>Ok(error(&e)),Err(_)=>Err(ProtectedRequestError::Internal),
    }
}
