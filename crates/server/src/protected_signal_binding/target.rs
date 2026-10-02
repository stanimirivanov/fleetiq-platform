//! Asset-centric binding attribution at explicit physical and recorded times.

use std::sync::Arc;

use axum::{
    Json,
    extract::{
        Path, Query, State,
        rejection::{PathRejection, QueryRejection},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use fleetiq_access::{ActorIdentityResolver, ExternalIdentityAuthenticator, TenantAccessPolicy};
use fleetiq_domain::{AssetId, SignalBindingId, TenantId, UnixTimeMillis};
use fleetiq_metadata::{
    SignalBindingRepository, SignalBindingSnapshotPageQuery, SignalBindingSnapshotQuery,
};
use serde::{Deserialize, Serialize};

use crate::protected_request::{
    BAD_REQUEST, INTERNAL, NOT_FOUND, authenticate, authorize_metadata_read, no_store, problem,
};

use super::{SignalBindingQueryState, SignalBindingResponse};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TargetSnapshotParameters {
    effective_at_ms: i64,
    known_at_ms: i64,
    limit: Option<u16>,
    after: Option<String>,
}

#[derive(Serialize)]
struct TargetSnapshotResponse<'a> {
    asset_id: &'a str,
    effective_at_ms: i64,
    known_at_ms: i64,
    bindings: Vec<SignalBindingResponse<'a>>,
    next_after: Option<&'a str>,
}

pub(super) async fn get_target_snapshot<A, R, P, Q>(
    State(state): State<Arc<SignalBindingQueryState<A, R, P, Q>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    parameters: Result<Query<TargetSnapshotParameters>, QueryRejection>,
) -> Response
where
    A: ExternalIdentityAuthenticator<Credential = String>,
    R: ActorIdentityResolver,
    P: TenantAccessPolicy,
    Q: SignalBindingRepository,
{
    let actor = match authenticate(&headers, &state.authentication).await {
        Ok(actor) => actor,
        Err(error) => return error.into_response(),
    };
    let Ok(Path((tenant, asset_id))) = path else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let (Ok(tenant), Ok(asset_id)) = (TenantId::new(tenant), AssetId::new(asset_id)) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let grant = match authorize_metadata_read(actor, tenant, &state.authorization).await {
        Ok(grant) => grant,
        Err(error) => return error.into_response(),
    };
    let Ok(Query(parameters)) = parameters else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let Ok(after) = parameters.after.map(SignalBindingId::new).transpose() else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let snapshot = SignalBindingSnapshotQuery::new(
        UnixTimeMillis::new(parameters.effective_at_ms),
        UnixTimeMillis::new(parameters.known_at_ms),
    );
    let Ok(limit) = u8::try_from(parameters.limit.unwrap_or(50)) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let Ok(query) = SignalBindingSnapshotPageQuery::new(snapshot, limit, after) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    match state
        .bindings
        .find_for_target_page(grant, &asset_id, &query)
        .await
    {
        Ok(Some(page)) => no_store(
            Json(TargetSnapshotResponse {
                asset_id: asset_id.as_str(),
                effective_at_ms: parameters.effective_at_ms,
                known_at_ms: parameters.known_at_ms,
                bindings: page
                    .bindings()
                    .iter()
                    .map(SignalBindingResponse::from)
                    .collect(),
                next_after: page.next_after().map(SignalBindingId::as_str),
            })
            .into_response(),
        ),
        Ok(None) => problem(StatusCode::NOT_FOUND, NOT_FOUND, "Resource not found"),
        Err(_) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            INTERNAL,
            "Internal server error",
        ),
    }
}
