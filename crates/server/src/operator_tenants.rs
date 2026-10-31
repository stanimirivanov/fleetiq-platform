//! Advisory current-access discovery, distinct from platform tenant administration.

use crate::protected_request::{
    BAD_REQUEST, INTERNAL, RequestAuthenticator, authenticate, no_store, problem,
    protected_method_not_allowed,
};
use axum::{
    Json, Router,
    extract::{Query, State, rejection::QueryRejection},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use fleetiq_access::{
    TenantAction, TenantDiscovery, TenantDiscoveryQuery, TenantDiscoveryRepository,
};
use fleetiq_domain::TenantId;
use fleetiq_postgres::PostgresTenantDiscoveryRepository;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;

struct DiscoveryState<A, R> {
    authentication: A,
    discovery: TenantDiscovery<R>,
}

fn discovery_router<A, R>(authentication: A, repository: R) -> Router
where
    A: RequestAuthenticator + 'static,
    R: TenantDiscoveryRepository + 'static,
{
    Router::new()
        .route(
            "/api/v1/me/tenants",
            get(list_tenants::<A, R>).fallback(protected_method_not_allowed),
        )
        .with_state(Arc::new(DiscoveryState {
            authentication,
            discovery: TenantDiscovery::new(repository),
        }))
}

/// Shares the listener's single authentication mode; no caller-selected actor.
pub(crate) fn postgres_operator_tenants_router<A: RequestAuthenticator + 'static>(
    pool: PgPool,
    authentication: A,
) -> Router {
    discovery_router(authentication, PostgresTenantDiscoveryRepository::new(pool))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Parameters {
    limit: Option<u16>,
    after: Option<String>,
}

#[derive(Serialize)]
struct TenantResponse<'a> {
    tenant_id: &'a str,
    actions: Vec<&'static str>,
}
#[derive(Serialize)]
struct PageResponse<'a> {
    tenants: Vec<TenantResponse<'a>>,
    next_after: Option<&'a str>,
}

const fn action_name(action: TenantAction) -> &'static str {
    match action {
        TenantAction::ReadMetadata => "metadata.read",
        TenantAction::ReadTelemetry => "telemetry.read",
        TenantAction::ManageMetadata => "metadata.manage",
        TenantAction::ManageAccess => "access.manage",
        TenantAction::ManageProtocols => "protocol.manage",
        TenantAction::ManageAnalytics => "analytics.manage",
        TenantAction::ReadAlerts => "alerts.read",
        TenantAction::ManageAlerts => "alerts.manage",
        TenantAction::AcknowledgeAlerts => "alerts.acknowledge",
        TenantAction::ReadOperations => "operations.read",
        TenantAction::ManageOperations => "operations.manage",
    }
}

async fn list_tenants<A: RequestAuthenticator, R: TenantDiscoveryRepository>(
    State(state): State<Arc<DiscoveryState<A, R>>>,
    headers: HeaderMap,
    parameters: Result<Query<Parameters>, QueryRejection>,
) -> Response {
    // Authenticate before reporting malformed parameters, as for metadata reads.
    let actor = match authenticate(&headers, &state.authentication).await {
        Ok(actor) => actor,
        Err(error) => return error.into_response(),
    };
    let Ok(Query(parameters)) = parameters else {
        return invalid_request();
    };
    let Ok(after) = parameters.after.map(TenantId::new).transpose() else {
        return invalid_request();
    };
    let Ok(query) = TenantDiscoveryQuery::new(parameters.limit.unwrap_or(50), after) else {
        return invalid_request();
    };
    match state.discovery.find_page(actor, &query).await {
        Ok(page) => no_store(
            Json(PageResponse {
                tenants: page
                    .tenants()
                    .iter()
                    .map(|entry| TenantResponse {
                        tenant_id: entry.tenant_id().as_str(),
                        actions: entry
                            .actions()
                            .iter()
                            .map(|action| action_name(*action))
                            .collect(),
                    })
                    .collect(),
                next_after: page.next_after().map(TenantId::as_str),
            })
            .into_response(),
        ),
        Err(_) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            INTERNAL,
            "Internal server error",
        ),
    }
}
fn invalid_request() -> Response {
    problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request")
}

#[cfg(test)]
mod tests;
