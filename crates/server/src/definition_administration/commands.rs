//! HTTP publication maps durable outcomes; SQL and retry comparison stay outside transport.
use super::{
    DefinitionState,
    dto::{PublicationBody, PublicationResponse},
    internal, invalid,
};
use crate::protected_request::{self, RequestAuthenticator};
use axum::{
    Extension, Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use fleetiq_access::ManageMetadata;
use fleetiq_metadata::{DefinitionKind, DefinitionPublicationError, DefinitionPublicationOutcome};
use fleetiq_postgres::PostgresRepositoryError;
use std::sync::Arc;
pub(super) async fn publish<A: RequestAuthenticator>(
    State(state): State<Arc<DefinitionState<A>>>,
    Extension(kind): Extension<DefinitionKind>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<PublicationBody>, JsonRejection>,
) -> Response {
    let authorization = match state
        .authorize::<ManageMetadata>(
            &headers,
            path.as_ref().ok().map(|Path(id)| id.as_str()),
            true,
        )
        .await
    {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let Ok(Json(body)) = body else {
        return invalid();
    };
    let Some(definition) = body.definition(kind) else {
        return invalid();
    };
    match state.publications.publish(authorization, &definition).await {
        Ok(DefinitionPublicationOutcome::Published) => protected_request::no_store(
            (
                StatusCode::CREATED,
                Json(PublicationResponse {
                    outcome: "published",
                }),
            )
                .into_response(),
        ),
        Ok(DefinitionPublicationOutcome::AlreadyPublished) => protected_request::no_store(
            Json(PublicationResponse {
                outcome: "already_published",
            })
            .into_response(),
        ),
        Ok(DefinitionPublicationOutcome::Conflict) => protected_request::problem(
            StatusCode::CONFLICT,
            "urn:fleetiq:problem:conflict",
            "Definition version contains different immutable content",
        ),
        Err(DefinitionPublicationError::Repository(
            PostgresRepositoryError::InvalidReference { .. },
        )) => protected_request::problem(
            StatusCode::NOT_FOUND,
            protected_request::NOT_FOUND,
            "Referenced resource not found",
        ),
        Err(_) => internal(),
    }
}
