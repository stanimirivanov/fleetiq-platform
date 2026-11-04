//! Tenant operational evidence, distinct from device diagnostics and health probes.
#[cfg(test)]
mod tests;
use crate::protected_request::{self, ProtectedRequestError, RequestAuthenticator};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::get,
};
use fleetiq_access::{ReadOperations, TenantAuthorizationError, TenantAuthorizationService};
use fleetiq_domain::TenantId;
use fleetiq_postgres::{PostgresTenantAccessPolicy, PostgresWorkDiagnostics};
use serde_json::json;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
struct Context<A> {
    authentication: A,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    diagnostics: PostgresWorkDiagnostics,
}
pub(crate) fn router<A: RequestAuthenticator + 'static>(pool: PgPool, authentication: A) -> Router {
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/work-diagnostics",
            get(find::<A>).fallback(protected_request::resource_method_not_allowed),
        )
        .with_state(Arc::new(Context {
            authentication,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
                pool.clone(),
            )),
            diagnostics: PostgresWorkDiagnostics::new(pool),
        }))
}
async fn find<A: RequestAuthenticator>(
    State(context): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<String>, axum::extract::rejection::PathRejection>,
) -> Result<Response, ProtectedRequestError> {
    let actor = protected_request::authenticate(&headers, &context.authentication).await?;
    let tenant = path
        .ok()
        .and_then(|p| TenantId::new(p.0).ok())
        .ok_or(ProtectedRequestError::Forbidden)?;
    let auth = context
        .authorization
        .authorize::<ReadOperations>(actor, tenant)
        .await
        .map_err(|e| match e {
            TenantAuthorizationError::Denied => ProtectedRequestError::Forbidden,
            TenantAuthorizationError::Policy(_) => ProtectedRequestError::Internal,
        })?;
    let snapshot = tokio::time::timeout(Duration::from_secs(5), context.diagnostics.snapshot(auth))
        .await
        .map_err(|_| ProtectedRequestError::Internal)?
        .map_err(|_| ProtectedRequestError::Internal)?;
    let queues:Vec<_>=snapshot.queues.into_iter().map(|q|json!({
        "kind":q.kind,"sampled":q.sampled,"eligible":q.eligible,"leased":q.leased,"failed":q.failed,"blocked":q.blocked,"deferred":q.deferred,
        "max_attempts":q.max_attempts,"oldest_sampled_eligible_at_ms":q.oldest_sampled_eligible_at_ms.map(|ms|ms.to_string()),
        "failure_categories":q.failure_categories,"truncated":q.truncated
    })).collect();
    Ok(protected_request::no_store(Json(json!({"observed_at_ms":snapshot.observed_at_ms.to_string(),"worker_liveness":"not_observed","notification_seeding":"not_observed","sample_limit":64,"queues":queues})).into_response()))
}
