//! Explicit operator intent; never condition resolution or notification suppression.
use crate::{
    mutation_guard::MutationGuard,
    protected_request::{self, ProtectedRequestError, RequestAuthenticator},
};
use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, Query, State as ExtractState,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::put,
};
use fleetiq_access::{AcknowledgeAlerts, TenantAuthorizationError, TenantAuthorizationService};
use fleetiq_analytics::AlertRuleId;
use fleetiq_domain::TenantId;
use fleetiq_postgres::{
    AlertAcknowledgement, AlertAcknowledgementError, PostgresAlertAcknowledgement,
    PostgresTenantAccessPolicy,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
#[cfg(test)]
mod tests;
struct Context<A> {
    authentication: A,
    mutation: MutationGuard,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    store: PostgresAlertAcknowledgement,
}
pub(crate) fn router<A: RequestAuthenticator + 'static>(
    pool: PgPool,
    authentication: A,
    mutation: MutationGuard,
) -> Router {
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/alerts/{rule_id}/revisions/{revision}/acknowledgement",
            put(acknowledge::<A>).fallback(method_not_allowed),
        )
        .layer(DefaultBodyLimit::max(1024))
        .with_state(Arc::new(Context {
            authentication,
            mutation,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
                pool.clone(),
            )),
            store: PostgresAlertAcknowledgement::new(pool),
        }))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    expected_version: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
type Target = Result<Path<(String, String, String)>, PathRejection>;
fn problem(status: StatusCode, title: &'static str) -> Response {
    protected_request::problem(status, "urn:fleetiq:problem:alert-acknowledgement", title)
}
async fn method_not_allowed() -> Response {
    let mut r = problem(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed");
    r.headers_mut()
        .insert(header::ALLOW, "PUT".parse().expect("static header"));
    r
}
pub(crate) fn receipt(r: &AlertAcknowledgement) -> Value {
    json!({"version":r.version.to_string(),"actor_id":r.actor.as_str(),"recorded_at_ms":r.recorded_at.get().to_string()})
}
async fn acknowledge<A: RequestAuthenticator>(
    ExtractState(c): ExtractState<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Target,
    query: Result<Query<Empty>, QueryRejection>,
    body: Result<Json<Command>, JsonRejection>,
) -> Response {
    let actor = match protected_request::authenticate(&headers, &c.authentication).await {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    if let Err(e) = c.mutation.check(&headers) {
        return e.into_response();
    }
    let Some(tenant) = path
        .as_ref()
        .ok()
        .and_then(|Path((t, _, _))| TenantId::new(t).ok())
    else {
        return ProtectedRequestError::Forbidden.into_response();
    };
    let auth = match c
        .authorization
        .authorize::<AcknowledgeAlerts>(actor, tenant)
        .await
    {
        Ok(a) => a,
        Err(TenantAuthorizationError::Denied) => {
            return ProtectedRequestError::Forbidden.into_response();
        }
        Err(TenantAuthorizationError::Policy(_)) => {
            return ProtectedRequestError::Internal.into_response();
        }
    };
    let (Ok(Path((_, rule, revision))), Ok(Query(_)), Ok(Json(command))) = (path, query, body)
    else {
        return problem(StatusCode::BAD_REQUEST, "Invalid acknowledgement request");
    };
    let parsed = || {
        let rule = AlertRuleId::new(rule).ok()?;
        let r = revision.parse::<u32>().ok()?;
        let v = command.expected_version.parse::<i64>().ok()?;
        (r > 0 && r.to_string() == revision && v > 0 && v.to_string() == command.expected_version)
            .then_some((rule, r, v))
    };
    let Some((rule, revision, version)) = parsed() else {
        return problem(
            StatusCode::BAD_REQUEST,
            "Invalid acknowledgement identity or version",
        );
    };
    match c.store.acknowledge(auth, &rule, revision, version).await {
        Ok(r) => protected_request::no_store(Json(receipt(&r)).into_response()),
        Err(AlertAcknowledgementError::Forbidden) => {
            ProtectedRequestError::Forbidden.into_response()
        }
        Err(AlertAcknowledgementError::Conflict) => problem(
            StatusCode::CONFLICT,
            "Alert acknowledgement version, phase or author conflicts",
        ),
        Err(_) => ProtectedRequestError::Internal.into_response(),
    }
}
