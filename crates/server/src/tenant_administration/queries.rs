//! Bounded catalogue and audit reads consume exact tenant-lifecycle evidence.
use super::{
    AdministrationState,
    dto::{
        AuditCursor, AuditEntry, AuditParameters, AuditResponse, CatalogParameters,
        CatalogResponse, TenantResponse, state_changed,
    },
    internal, invalid,
};
use crate::protected_request::{RequestAuthenticator, no_store};
use axum::{
    Json,
    extract::{Query, State, rejection::QueryRejection},
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use fleetiq_access::ManageTenantLifecycle;
use fleetiq_domain::{TenantId, TenantLifecycleChangeId, UnixTimeMillis};
use fleetiq_identity::{
    TenantCatalogCursor, TenantCatalogPageSize, TenantCatalogQuery, TenantLifecycleAuditCursor,
    TenantLifecycleAuditPageSize, TenantLifecycleAuditQuery, TenantLifecycleChangeKind,
    TenantLifecycleState,
};
use std::sync::Arc;

pub(super) async fn catalog<A: RequestAuthenticator>(
    State(state): State<Arc<AdministrationState<A>>>,
    headers: HeaderMap,
    parameters: Result<Query<CatalogParameters>, QueryRejection>,
) -> Response {
    let authorization = match state
        .authorize::<ManageTenantLifecycle>(&headers, false)
        .await
    {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let Ok(Query(parameters)) = parameters else {
        return invalid();
    };
    let Ok(limit) = TenantCatalogPageSize::new(parameters.limit.unwrap_or(50)) else {
        return invalid();
    };
    let Ok(after) = parameters.after.map(TenantId::new).transpose() else {
        return invalid();
    };
    let query = TenantCatalogQuery::new(limit, after.map(TenantCatalogCursor::new));
    match state.catalog.find_page(authorization, &query).await {
        Ok(page) => no_store(
            Json(CatalogResponse {
                tenants: page
                    .entries()
                    .iter()
                    .map(|entry| TenantResponse {
                        tenant_id: entry.tenant_id().as_str(),
                        state: match entry.state() {
                            TenantLifecycleState::Active => "active",
                            TenantLifecycleState::Suspended => "suspended",
                        },
                    })
                    .collect(),
                next_after: page.next().map(|cursor| cursor.tenant_id().as_str()),
            })
            .into_response(),
        ),
        Err(_) => internal(),
    }
}
pub(super) async fn audit<A: RequestAuthenticator>(
    State(state): State<Arc<AdministrationState<A>>>,
    headers: HeaderMap,
    parameters: Result<Query<AuditParameters>, QueryRejection>,
) -> Response {
    let authorization = match state
        .authorize::<ManageTenantLifecycle>(&headers, false)
        .await
    {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let Ok(Query(parameters)) = parameters else {
        return invalid();
    };
    let Ok(limit) = TenantLifecycleAuditPageSize::new(parameters.limit.unwrap_or(50)) else {
        return invalid();
    };
    let after = match (
        parameters.before_recorded_at_ms,
        parameters.before_change_id,
    ) {
        (None, None) => None,
        (Some(time), Some(id)) => {
            let Ok(id) = TenantLifecycleChangeId::new(id) else {
                return invalid();
            };
            Some(TenantLifecycleAuditCursor::new(
                UnixTimeMillis::new(time),
                id,
            ))
        }
        _ => return invalid(),
    };
    match state
        .audit
        .find_page(authorization, &TenantLifecycleAuditQuery::new(limit, after))
        .await
    {
        Ok(page) => no_store(
            Json(AuditResponse {
                changes: page
                    .entries()
                    .iter()
                    .map(|entry| AuditEntry {
                        change_id: entry.change_id().as_str(),
                        administrator_actor_id: entry.administrator_actor_id().as_str(),
                        tenant_id: entry.tenant_id().as_str(),
                        kind: match entry.kind() {
                            TenantLifecycleChangeKind::Activate => "activate",
                            TenantLifecycleChangeKind::Suspend => "suspend",
                        },
                        recorded_at_ms: entry.recorded_at().get(),
                        state_changed: state_changed(entry.effect()),
                    })
                    .collect(),
                next_before: page.next().map(|cursor| AuditCursor {
                    recorded_at_ms: cursor.recorded_at().get(),
                    change_id: cursor.change_id().as_str(),
                }),
            })
            .into_response(),
        ),
        Err(_) => internal(),
    }
}
