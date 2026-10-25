use super::{
    State,
    dto::{self, ChangeBody, RouteBody},
};
use crate::protected_request::{self, ProtectedRequestError, RequestAuthenticator};
use axum::{
    Json,
    extract::{
        Path, State as ExtractState,
        rejection::{JsonRejection, PathRejection},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use fleetiq_ingestion::ProtocolRouteWrite;
use std::sync::Arc;
type RoutePath = Result<Path<(String, String, String)>, PathRejection>;
fn problem(status: StatusCode, title: &'static str) -> Response {
    protected_request::problem(status, "urn:fleetiq:problem:protocol-route", title)
}
pub(super) async fn find<A: RequestAuthenticator>(
    ExtractState(state): ExtractState<Arc<State<A>>>,
    headers: HeaderMap,
    path: RoutePath,
) -> Response {
    let auth = match state
        .authorize(
            &headers,
            path.as_ref()
                .ok()
                .map(|Path((tenant, _, _))| tenant.as_str()),
            false,
        )
        .await
    {
        Ok(auth) => auth,
        Err(error) => return error.into_response(),
    };
    let Ok(Path((_, device, channel))) = path else {
        return problem(StatusCode::BAD_REQUEST, "Invalid route identity");
    };
    let Some((device, channel)) = dto::identity(&device, &channel) else {
        return problem(StatusCode::BAD_REQUEST, "Invalid route identity");
    };
    match state.routes.find(auth, &device, &channel).await {
        Ok(Some(route)) => {
            protected_request::no_store(Json(RouteBody::from(&route)).into_response())
        }
        Ok(None) => problem(StatusCode::NOT_FOUND, "Protocol route not found"),
        Err(_) => ProtectedRequestError::Internal.into_response(),
    }
}
pub(super) async fn change<A: RequestAuthenticator>(
    ExtractState(state): ExtractState<Arc<State<A>>>,
    headers: HeaderMap,
    path: RoutePath,
    body: Result<Json<ChangeBody>, JsonRejection>,
) -> Response {
    let auth = match state
        .authorize(
            &headers,
            path.as_ref()
                .ok()
                .map(|Path((tenant, _, _))| tenant.as_str()),
            true,
        )
        .await
    {
        Ok(auth) => auth,
        Err(error) => return error.into_response(),
    };
    let (Ok(Path((_, device, channel))), Ok(Json(body))) = (path, body) else {
        return problem(StatusCode::BAD_REQUEST, "Invalid route request");
    };
    let Some((device, channel)) = dto::identity(&device, &channel) else {
        return problem(StatusCode::BAD_REQUEST, "Invalid route identity");
    };
    let expected = body.expected_revision;
    let Ok(selection) = body.selection() else {
        return problem(StatusCode::BAD_REQUEST, "Invalid route selection");
    };
    let now = match protected_request::now() {
        Ok(now) => now,
        Err(error) => return error.into_response(),
    };
    match state
        .routes
        .change(auth, device, channel, expected, selection, now)
        .await
    {
        Ok(ProtocolRouteWrite::Committed(route)) => {
            protected_request::no_store(Json(RouteBody::from(&route)).into_response())
        }
        Ok(ProtocolRouteWrite::Conflict) => {
            problem(StatusCode::CONFLICT, "Route revision conflicts")
        }
        Ok(ProtocolRouteWrite::Unavailable) => {
            problem(StatusCode::CONFLICT, "Route prerequisites unavailable")
        }
        Err(_) => ProtectedRequestError::Internal.into_response(),
    }
}
