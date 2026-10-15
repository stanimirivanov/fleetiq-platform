//! Authorized bounded reads; current grants and immutable history remain separate.
use super::{
    AdministrationState,
    dto::{
        Action, AuditCursor, AuditEntry, AuditParameters, AuditResponse, CatalogParameters,
        CatalogResponse, GrantResponse, kind_name, state_changed,
    },
    internal, invalid,
};
use crate::protected_request::{RequestAuthenticator, no_store};
use axum::{
    Json,
    extract::{
        Path, Query, State,
        rejection::{PathRejection, QueryRejection},
    },
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use fleetiq_domain::ActorId;
use fleetiq_identity::{
    PlatformAccessAuditPageSize, PlatformAccessAuditQuery, PlatformGrantCatalogPageSize,
    PlatformGrantCatalogQuery,
};
use std::sync::Arc;

pub(super) async fn catalog<A: RequestAuthenticator>(
    State(state): State<Arc<AdministrationState<A>>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    parameters: Result<Query<CatalogParameters>, QueryRejection>,
) -> Response {
    let authorization = match state.authorize(&headers, false).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let (Ok(Path(actor)), Ok(Query(parameters))) = (path, parameters) else {
        return invalid();
    };
    let Ok(actor) = ActorId::new(actor) else {
        return invalid();
    };
    let Ok(limit) = PlatformGrantCatalogPageSize::new(parameters.limit.unwrap_or(50)) else {
        return invalid();
    };
    let after = parameters.cursor();
    match state
        .catalog
        .find_page(
            authorization,
            &PlatformGrantCatalogQuery::new(actor, limit, after),
        )
        .await
    {
        Ok(page) => no_store(
            Json(CatalogResponse {
                grants: page
                    .entries()
                    .iter()
                    .map(|entry| GrantResponse {
                        action: Action::from(entry.action()),
                    })
                    .collect(),
                next_after: page.next().map(|cursor| Action::from(cursor.action())),
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
    let Ok(limit) = PlatformAccessAuditPageSize::new(parameters.limit.unwrap_or(50)) else {
        return invalid();
    };
    let Ok(after) = parameters.cursor() else {
        return invalid();
    };
    match state
        .audit
        .find_page(authorization, &PlatformAccessAuditQuery::new(limit, after))
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
                        action: Action::from(entry.action()),
                        kind: kind_name(entry.kind()),
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
