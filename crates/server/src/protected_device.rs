use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State, rejection::PathRejection},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use fleetiq_access::{
    ActorIdentityResolver, CallerAuthenticationService, ExternalIdentityAuthenticator,
    TenantAccessPolicy, TenantAuthorizationService,
};
use fleetiq_domain::{AssetId, CommunicationDevice, TenantId};
use fleetiq_metadata::{DeviceApplicationService, DeviceRepository};
use fleetiq_postgres::{
    PostgresActorIdentityResolver, PostgresDeviceRepository, PostgresTenantAccessPolicy,
};
use serde::Serialize;
use sqlx::PgPool;

use crate::{
    bearer_authentication::ConfiguredBearerAuthenticator,
    config::BearerIdentityConfig,
    protected_request::{
        BAD_REQUEST, INTERNAL, NOT_FOUND, authenticate, authorize_metadata_read, no_store, problem,
        protected_method_not_allowed,
    },
};

struct DeviceQueryState<A, R, P, Q> {
    authentication: CallerAuthenticationService<A, R>,
    authorization: TenantAuthorizationService<P>,
    devices: DeviceApplicationService<Q>,
}

/// Builds the protected device-detail route from independently testable ports.
///
/// A device is an asset's communication capability, not a second asset
/// identity. The handler uses the same per-request authentication and current
/// metadata-read policy as the asset route.
fn device_router<A, R, P, Q>(
    authentication: CallerAuthenticationService<A, R>,
    authorization: TenantAuthorizationService<P>,
    devices: DeviceApplicationService<Q>,
) -> Router
where
    A: ExternalIdentityAuthenticator<Credential = String> + 'static,
    R: ActorIdentityResolver + 'static,
    P: TenantAccessPolicy + 'static,
    Q: DeviceRepository + 'static,
{
    let state = Arc::new(DeviceQueryState {
        authentication,
        authorization,
        devices,
    });
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/assets/{asset_id}/device",
            get(get_device::<A, R, P, Q>).fallback(protected_method_not_allowed),
        )
        .with_state(state)
}

/// Wires the configured credential and PostgreSQL adapters to device lookup.
pub(crate) fn postgres_device_router(pool: PgPool, bearer: BearerIdentityConfig) -> Router {
    device_router(
        CallerAuthenticationService::new(
            ConfiguredBearerAuthenticator::new(bearer),
            PostgresActorIdentityResolver::new(pool.clone()),
        ),
        TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone())),
        DeviceApplicationService::new(PostgresDeviceRepository::new(pool)),
    )
}

async fn get_device<A, R, P, Q>(
    State(state): State<Arc<DeviceQueryState<A, R, P, Q>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response
where
    A: ExternalIdentityAuthenticator<Credential = String>,
    R: ActorIdentityResolver,
    P: TenantAccessPolicy,
    Q: DeviceRepository,
{
    let actor = match authenticate(&headers, &state.authentication).await {
        Ok(actor) => actor,
        Err(error) => return error.into_response(),
    };
    let Ok(Path((tenant, asset))) = path else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let (Ok(tenant), Ok(asset_id)) = (TenantId::new(tenant), AssetId::new(asset)) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let grant = match authorize_metadata_read(actor, tenant, &state.authorization).await {
        Ok(grant) => grant,
        Err(error) => return error.into_response(),
    };
    match state.devices.find_by_asset(grant, &asset_id).await {
        Ok(Some(device)) => no_store(Json(DeviceResponse::from(&device)).into_response()),
        Ok(None) => problem(StatusCode::NOT_FOUND, NOT_FOUND, "Resource not found"),
        Err(_) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            INTERNAL,
            "Internal server error",
        ),
    }
}

#[derive(Serialize)]
struct DeviceResponse<'a> {
    tenant_id: &'a str,
    asset_id: &'a str,
    endpoints: Vec<DeviceEndpointResponse<'a>>,
}

impl<'a> From<&'a CommunicationDevice> for DeviceResponse<'a> {
    fn from(device: &'a CommunicationDevice) -> Self {
        Self {
            tenant_id: device.tenant_id().as_str(),
            asset_id: device.asset_id().as_str(),
            endpoints: device
                .endpoints()
                .iter()
                .map(|endpoint| DeviceEndpointResponse {
                    id: endpoint.id().as_str(),
                    kind: endpoint.kind().as_str(),
                    name: endpoint.name().as_str(),
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct DeviceEndpointResponse<'a> {
    id: &'a str,
    kind: &'a str,
    name: &'a str,
}

#[cfg(test)]
mod tests;
