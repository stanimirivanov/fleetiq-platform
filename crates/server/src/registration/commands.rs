//! HTTP results preserve collision, missing-reference and ambiguous storage outcomes.
use super::{
    RegistrationState,
    dto::{AssetBody, DeviceBody, RegistrationResponse},
    invalid,
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
use fleetiq_metadata::{MetadataRegistration, RegistrationError, RegistrationOutcome};
use fleetiq_postgres::PostgresRepositoryError;
use std::sync::Arc;

pub(super) async fn asset<A: RequestAuthenticator>(
    State(state): State<Arc<RegistrationState<A>>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<AssetBody>, JsonRejection>,
) -> Response {
    let authorization = match state
        .authorize(&headers, path.as_ref().ok().map(|Path(id)| id.as_str()))
        .await
    {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let Ok(Json(body)) = body else {
        return invalid();
    };
    let Some(registration) = body.registration(authorization.tenant_id().clone()) else {
        return invalid();
    };
    execute(&state, authorization, &registration).await
}
pub(super) async fn device<A: RequestAuthenticator>(
    State(state): State<Arc<RegistrationState<A>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Result<Json<DeviceBody>, JsonRejection>,
) -> Response {
    let authorization = match state
        .authorize(
            &headers,
            path.as_ref().ok().map(|Path((id, _))| id.as_str()),
        )
        .await
    {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let (Ok(Path((_, asset))), Ok(Json(body))) = (path, body) else {
        return invalid();
    };
    let Some(registration) = body.registration(authorization.tenant_id().clone(), asset) else {
        return invalid();
    };
    execute(&state, authorization, &registration).await
}
async fn execute<A: RequestAuthenticator>(
    state: &RegistrationState<A>,
    authorization: AuthorizedTenant<ManageMetadata>,
    registration: &MetadataRegistration,
) -> Response {
    let time = match protected_request::now() {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    match state
        .registrations
        .register(authorization, registration, time)
        .await
    {
        Ok(RegistrationOutcome::Registered(receipt)) => protected_request::no_store(
            (
                StatusCode::CREATED,
                Json(RegistrationResponse::new("registered", &receipt)),
            )
                .into_response(),
        ),
        Ok(RegistrationOutcome::AlreadyRegistered(receipt)) => protected_request::no_store(
            Json(RegistrationResponse::new("already_registered", &receipt)).into_response(),
        ),
        Ok(RegistrationOutcome::Conflict) => protected_request::problem(
            StatusCode::CONFLICT,
            "urn:fleetiq:problem:conflict",
            "Registration identity is occupied",
        ),
        Err(RegistrationError::Repository(PostgresRepositoryError::InvalidReference {
            ..
        })) => protected_request::problem(
            StatusCode::NOT_FOUND,
            protected_request::NOT_FOUND,
            "Referenced resource not found",
        ),
        Err(_) => ProtectedRequestError::Internal.into_response(),
    }
}
