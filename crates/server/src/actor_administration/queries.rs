//! Bounded catalogue and audit reads consume exact identity-management evidence.
use super::{
    AdministrationState,
    dto::{
        ActorResponse, AuditCursor, AuditEntry, AuditParameters, AuditResponse, CatalogParameters,
        CatalogResponse, state_changed,
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
use fleetiq_domain::{ActorId, ActorLifecycleChangeId, UnixTimeMillis};
use fleetiq_identity::{
    ActorCatalogCursor, ActorCatalogPageSize, ActorCatalogQuery, ActorLifecycleAuditCursor,
    ActorLifecycleAuditPageSize, ActorLifecycleAuditQuery, ActorLifecycleChangeKind,
    ActorLifecycleState,
};
use std::sync::Arc;

pub(super) async fn catalog<A: RequestAuthenticator>(
    State(state): State<Arc<AdministrationState<A>>>,
    headers: HeaderMap,
    parameters: Result<Query<CatalogParameters>, QueryRejection>,
) -> Response {
    let authorization = match state.authorize(&headers, false).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let Ok(Query(parameters)) = parameters else {
        return invalid();
    };
    let Ok(limit) = ActorCatalogPageSize::new(parameters.limit.unwrap_or(50)) else {
        return invalid();
    };
    let Ok(after) = parameters.after.map(ActorId::new).transpose() else {
        return invalid();
    };
    let query = ActorCatalogQuery::new(limit, after.map(ActorCatalogCursor::new));
    match state.catalog.find_page(authorization, &query).await {
        Ok(page) => no_store(
            Json(CatalogResponse {
                actors: page
                    .entries()
                    .iter()
                    .map(|entry| ActorResponse {
                        actor_id: entry.actor_id().as_str(),
                        state: match entry.state() {
                            ActorLifecycleState::Active => "active",
                            ActorLifecycleState::Deactivated => "deactivated",
                        },
                    })
                    .collect(),
                next_after: page.next().map(|cursor| cursor.actor_id().as_str()),
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
    let authorization = match state.authorize(&headers, false).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let Ok(Query(parameters)) = parameters else {
        return invalid();
    };
    let Ok(limit) = ActorLifecycleAuditPageSize::new(parameters.limit.unwrap_or(50)) else {
        return invalid();
    };
    let after = match (
        parameters.before_recorded_at_ms,
        parameters.before_change_id,
    ) {
        (None, None) => None,
        (Some(time), Some(id)) => {
            let Ok(id) = ActorLifecycleChangeId::new(id) else {
                return invalid();
            };
            Some(ActorLifecycleAuditCursor::new(
                UnixTimeMillis::new(time),
                id,
            ))
        }
        _ => return invalid(),
    };
    match state
        .audit
        .find_page(authorization, &ActorLifecycleAuditQuery::new(limit, after))
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
                        target_actor_id: entry.target_actor_id().as_str(),
                        kind: match entry.kind() {
                            ActorLifecycleChangeKind::Activate => "activate",
                            ActorLifecycleChangeKind::Deactivate => "deactivate",
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
