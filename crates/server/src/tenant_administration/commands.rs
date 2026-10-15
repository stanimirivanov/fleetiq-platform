//! Thin command adapters. Trusted receipt time and administrator never come from JSON.
use super::{
    AdministrationState, conflict,
    dto::{LifecycleBody, LifecycleResponse, ProvisionBody, ProvisionResponse, state_changed},
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
use fleetiq_access::{ManageTenantLifecycle, ProvisionTenant};
use fleetiq_identity::{
    TenantLifecycleAdministrationError, TenantLifecycleChangeOutcome, TenantProvisioningError,
    TenantProvisioningOutcome,
};
use fleetiq_postgres::{
    PostgresRepositoryError, PostgresTenantLifecycleAdministrationError as LifecycleError,
    PostgresTenantProvisioningError as ProvisionError,
};
use std::sync::Arc;

pub(super) async fn provision<A: RequestAuthenticator>(
    State(state): State<Arc<AdministrationState<A>>>,
    headers: HeaderMap,
    body: Result<Json<ProvisionBody>, JsonRejection>,
) -> Response {
    let authorization = match state.authorize::<ProvisionTenant>(&headers, true).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let Ok(Json(body)) = body else {
        return invalid();
    };
    let Ok(time) = now() else {
        return internal();
    };
    let Some(request) = body.request(time) else {
        return invalid();
    };
    match state.provisioning.provision(authorization, request).await {
        Ok(outcome) => {
            let (status, outcome) = match outcome {
                TenantProvisioningOutcome::Provisioned => (StatusCode::CREATED, "provisioned"),
                TenantProvisioningOutcome::AlreadyProvisioned => {
                    (StatusCode::OK, "already_provisioned")
                }
            };
            protected_request::no_store(
                (status, Json(ProvisionResponse { outcome })).into_response(),
            )
        }
        Err(TenantProvisioningError::Repository(error)) => match error {
            ProvisionError::IdentityCollision { .. }
            | ProvisionError::Repository(PostgresRepositoryError::Conflict { .. }) => conflict(),
            ProvisionError::Repository(PostgresRepositoryError::InvalidReference { .. }) => {
                not_found()
            }
            ProvisionError::Repository(_) => internal(),
        },
    }
}
pub(super) async fn lifecycle<A: RequestAuthenticator>(
    State(state): State<Arc<AdministrationState<A>>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<LifecycleBody>, JsonRejection>,
) -> Response {
    let authorization = match state
        .authorize::<ManageTenantLifecycle>(&headers, true)
        .await
    {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let (Ok(Path(tenant)), Ok(Json(body))) = (path, body) else {
        return invalid();
    };
    let Ok(time) = now() else {
        return internal();
    };
    let Some(request) = body.request(tenant, time) else {
        return invalid();
    };
    match state.lifecycle.apply(authorization, request).await {
        Ok(outcome) => {
            let (outcome, effect) = match outcome {
                TenantLifecycleChangeOutcome::Recorded(effect) => ("recorded", effect),
                TenantLifecycleChangeOutcome::AlreadyRecorded(effect) => {
                    ("already_recorded", effect)
                }
            };
            protected_request::no_store(
                Json(LifecycleResponse {
                    outcome,
                    state_changed: state_changed(effect),
                })
                .into_response(),
            )
        }
        Err(TenantLifecycleAdministrationError::Repository(error)) => match error {
            LifecycleError::TenantNotFound { .. } => not_found(),
            LifecycleError::IdentityCollision { .. } => conflict(),
            LifecycleError::Repository(_) => internal(),
        },
    }
}

fn not_found() -> Response {
    protected_request::problem(
        StatusCode::NOT_FOUND,
        protected_request::NOT_FOUND,
        "Resource not found",
    )
}
