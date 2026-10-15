//! One thin adapter for grant/revoke commands; persistence resolves retries before mutation.
use super::{
    AdministrationState, conflict,
    dto::{ChangeBody, ChangeResponse, state_changed},
    internal, invalid,
};
use crate::protected_request::{self, RequestAuthenticator, now};
use axum::{
    Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use fleetiq_identity::{PlatformAccessAdministrationError, PlatformAccessChangeOutcome};
use fleetiq_postgres::{
    PostgresPlatformAccessAdministrationError as ChangeError, PostgresRepositoryError,
};
use std::sync::Arc;

pub(super) async fn change<A: RequestAuthenticator>(
    State(state): State<Arc<AdministrationState<A>>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<ChangeBody>, JsonRejection>,
) -> Response {
    let authorization = match state.authorize(&headers, true).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let (Ok(Path(actor)), Ok(Json(body))) = (path, body) else {
        return invalid();
    };
    let Ok(time) = now() else {
        return internal();
    };
    let Some(request) = body.request(actor, time) else {
        return invalid();
    };
    match state.changes.apply(authorization, request).await {
        Ok(outcome) => {
            let (outcome, effect) = match outcome {
                PlatformAccessChangeOutcome::Recorded(effect) => ("recorded", effect),
                PlatformAccessChangeOutcome::AlreadyRecorded(effect) => {
                    ("already_recorded", effect)
                }
            };
            protected_request::no_store(
                Json(ChangeResponse {
                    outcome,
                    state_changed: state_changed(effect),
                })
                .into_response(),
            )
        }
        Err(PlatformAccessAdministrationError::Repository(error)) => match error {
            ChangeError::IdentityCollision { .. }
            | ChangeError::LastPlatformAdministrator { .. } => conflict(),
            ChangeError::Repository(PostgresRepositoryError::InvalidReference { .. }) => {
                protected_request::problem(
                    StatusCode::NOT_FOUND,
                    protected_request::NOT_FOUND,
                    "Resource not found",
                )
            }
            ChangeError::Repository(_) => internal(),
        },
    }
}
