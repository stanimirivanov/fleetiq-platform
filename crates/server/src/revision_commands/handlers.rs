//! Authorization precedes input errors; receipts never reinterpret a retry.
use super::{
    RevisionState,
    dto::{BindingBody, RelationshipBody},
};
use crate::protected_request::{self, ProtectedRequestError, RequestAuthenticator};
use axum::{
    Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use fleetiq_access::{AuthorizedTenant, ManageMetadata};
use fleetiq_metadata::{MetadataRevision, RevisionCommandError, RevisionOutcome, RevisionReceipt};
use fleetiq_postgres::PostgresRepositoryError;
use serde::Serialize;
use std::sync::Arc;
type RevisionPath = Result<Path<(String, String, String)>, PathRejection>;

pub(super) async fn relationship<A: RequestAuthenticator>(
    State(state): State<Arc<RevisionState<A>>>,
    headers: HeaderMap,
    path: RevisionPath,
    body: Result<Json<RelationshipBody>, JsonRejection>,
) -> Response {
    let grant = match state
        .authorize(
            &headers,
            path.as_ref().ok().map(|Path((t, _, _))| t.as_str()),
        )
        .await
    {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let (Ok(Path((_, id, number))), Ok(Json(body))) = (path, body) else {
        return invalid();
    };
    let Some(number) = protected_request::parse_record_revision(&number) else {
        return invalid();
    };
    let time = match protected_request::now() {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let Some(value) = body.domain(grant.tenant_id().clone(), id, number, time) else {
        return invalid();
    };
    execute(&state, grant, &value).await
}
pub(super) async fn binding<A: RequestAuthenticator>(
    State(state): State<Arc<RevisionState<A>>>,
    headers: HeaderMap,
    path: RevisionPath,
    body: Result<Json<BindingBody>, JsonRejection>,
) -> Response {
    let grant = match state
        .authorize(
            &headers,
            path.as_ref().ok().map(|Path((t, _, _))| t.as_str()),
        )
        .await
    {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let (Ok(Path((_, id, number))), Ok(Json(body))) = (path, body) else {
        return invalid();
    };
    let Some(number) = protected_request::parse_record_revision(&number) else {
        return invalid();
    };
    let time = match protected_request::now() {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let Some(value) = body.domain(grant.tenant_id().clone(), id, number, time) else {
        return invalid();
    };
    execute(&state, grant, &value).await
}
async fn execute<A: RequestAuthenticator>(
    state: &RevisionState<A>,
    grant: AuthorizedTenant<ManageMetadata>,
    value: &MetadataRevision,
) -> Response {
    match state.revisions.append(grant, value).await {
        Ok(RevisionOutcome::Appended(receipt)) => {
            success(StatusCode::CREATED, "appended", value, &receipt)
        }
        Ok(RevisionOutcome::AlreadyAppended(receipt)) => {
            success(StatusCode::OK, "already_appended", value, &receipt)
        }
        Ok(RevisionOutcome::Conflict) => protected_request::problem(
            StatusCode::CONFLICT,
            "urn:fleetiq:problem:conflict",
            "Revision conflicts with recorded history",
        ),
        Err(RevisionCommandError::Repository(PostgresRepositoryError::InvalidReference {
            ..
        })) => protected_request::problem(
            StatusCode::NOT_FOUND,
            protected_request::NOT_FOUND,
            "Referenced resource not found",
        ),
        Err(_) => ProtectedRequestError::Internal.into_response(),
    }
}
#[derive(Serialize)]
struct Receipt<'a> {
    outcome: &'static str,
    id: &'a str,
    revision: String,
    recorded_by: &'a str,
    recorded_at_ms: i64,
}
fn success(
    status: StatusCode,
    outcome: &'static str,
    value: &MetadataRevision,
    receipt: &RevisionReceipt,
) -> Response {
    protected_request::no_store(
        (
            status,
            Json(Receipt {
                outcome,
                id: value.id(),
                revision: value.revision().get().to_string(),
                recorded_by: receipt.actor_id().as_str(),
                recorded_at_ms: receipt.recorded_at().get(),
            }),
        )
            .into_response(),
    )
}
fn invalid() -> Response {
    protected_request::problem(
        StatusCode::BAD_REQUEST,
        protected_request::BAD_REQUEST,
        "Invalid request",
    )
}
